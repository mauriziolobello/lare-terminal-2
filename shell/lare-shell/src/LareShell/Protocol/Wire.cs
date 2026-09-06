using System.Text.Json;
using System.Text.Json.Nodes;

namespace LareShell.Protocol;

// ── Messaggi orchestratore → host ─────────────────────────────────────────────
// Un record per variante, con una classe base astratta: è l'equivalente C# dell'enum
// `ServerMsg` di Rust (crates/protocol). Il consumatore fa `switch (msg) { case Chunk c: … }`
// — pattern matching sui tipi, come il `match` di Rust. Solo le varianti che la shell
// gestisce hanno un record proprio; tutto il resto diventa `Unknown`.

internal abstract record ServerMessage;

internal sealed record ServerInfo(string Version, string AiProvider) : ServerMessage;

internal sealed record Chunk(string Id, string Content) : ServerMessage;

internal sealed record Done(string Id, int? ExitCode) : ServerMessage;

/// <summary>`error` sul filo. Si chiama TurnError e non Error per non confondersi con
/// <c>System.Exception</c>/la parola chiave dei log.</summary>
internal sealed record TurnError(string Id, string Code, string Message) : ServerMessage;

/// <summary>Gate ADR-007: <c>Id</c> è opaco (non è il turn_id), <c>Commands</c> può avere più righe.</summary>
internal sealed record ToolConfirmRequest(string Id, string Commands) : ServerMessage;

internal sealed record ExecInShell(string TurnId, string ExecId, string Command, bool Capture) : ServerMessage;

internal sealed record Heartbeat(string Id) : ServerMessage;

internal sealed record Pong(long Ts) : ServerMessage;

/// <summary>Tipo non gestito dalla shell (es. messaggi v1 per la ui): il loop lo consegna e va avanti.</summary>
internal sealed record Unknown(string Type) : ServerMessage;

/// <summary>Sintetico: lo accoda <c>OrchestratorClient</c> quando la connessione cade, così il
/// consumatore (thread del REPL) lo scopre leggendo il canale come ogni altro messaggio.</summary>
internal sealed record Disconnected(string Reason) : ServerMessage;

internal sealed class WireException : Exception
{
    public WireException(string message) : base(message) { }

    public WireException(string message, Exception inner) : base(message, inner) { }
}

/// <summary>
/// Traduzione fra il JSON del protocollo (snake_case, campo discriminante <c>type</c>) e i
/// record C#. Nessuna rete qui: funzioni pure, testabili senza socket.
/// </summary>
internal static class Wire
{
    public static ServerMessage Parse(string json)
    {
        JsonObject obj;
        try
        {
            obj = JsonNode.Parse(json) as JsonObject ?? throw new WireException("il messaggio non è un oggetto JSON");
        }
        catch (JsonException ex)
        {
            throw new WireException("JSON non valido: " + ex.Message, ex);
        }

        string type = Str(obj, "type");
        return type switch
        {
            "server_info" => new ServerInfo(Str(obj, "version"), Str(obj, "ai_provider")),
            "chunk" => new Chunk(Str(obj, "id"), Str(obj, "content")),
            "done" => new Done(Str(obj, "id"), NullableInt(obj, "exit_code")),
            "error" => new TurnError(Str(obj, "id"), Str(obj, "code"), Str(obj, "message")),
            "tool_confirm_request" => new ToolConfirmRequest(Str(obj, "id"), Str(obj, "commands")),
            "exec_in_shell" => new ExecInShell(Str(obj, "turn_id"), Str(obj, "exec_id"), Str(obj, "command"), Bool(obj, "capture")),
            "heartbeat" => new Heartbeat(Str(obj, "id")),
            "pong" => new Pong(Long(obj, "ts")),
            _ => new Unknown(type),
        };
    }

    // ── Costruttori dei messaggi host → orchestratore ───────────────────────
    // Tipi anonimi con i nomi snake_case scritti a mano: il JSON che ne esce è esattamente
    // quello che serde si aspetta; non serve una naming policy né classi dedicate.

    /// <summary><c>version</c> è la SOLA versione ("2.0.0", HostInfo.Version): la riga di /ping la
    /// stampa come <c>| lare-shell | 2.0.0 | …</c> (orchestrator/ping.rs), il nome lo mette lui.</summary>
    public static string Hello(string token, string sessionId, string cwd, string version) =>
        JsonSerializer.Serialize(new { type = "hello", token, role = "shell", session_id = sessionId, cwd, version });

    /// <summary><c>input_mode</c>/<c>command_type</c> sono obbligatori lato Rust (nessun
    /// <c>#[serde(default)]</c>): fissi a <c>keyboard</c>/<c>auto</c> — la shell non ha voce né
    /// classificazione OS/NL (il pre-router della shell decide dal testo). <c>web_search</c> false:
    /// per la shell vale la casella di /config (piano 2a).</summary>
    public static string Command(string id, string input, string cwd) =>
        JsonSerializer.Serialize(new { type = "command", id, input, input_mode = "keyboard", command_type = "auto", cwd, web_search = false });

    public static string ToolConfirmResponse(string id, bool accept) =>
        JsonSerializer.Serialize(new { type = "tool_confirm_response", id, accept });

    public static string ExecResult(string turnId, string execId, int exitCode, string output, string cwd) =>
        JsonSerializer.Serialize(new { type = "exec_result", turn_id = turnId, exec_id = execId, exit_code = exitCode, output, cwd });

    public static string CancelCommand(string id) =>
        JsonSerializer.Serialize(new { type = "cancel_command", id });

    public static string Ping(long ts) =>
        JsonSerializer.Serialize(new { type = "ping", ts });

    // ── Helper di lettura: campo mancante o del tipo sbagliato → WireException ──

    private static string Str(JsonObject obj, string name)
    {
        JsonNode? node = obj[name];
        if (node is null)
        {
            throw new WireException("campo mancante: " + name);
        }

        try
        {
            return node.GetValue<string>();
        }
        catch (Exception ex) when (ex is InvalidOperationException or FormatException)
        {
            throw new WireException("campo " + name + " non è una stringa", ex);
        }
    }

    private static bool Bool(JsonObject obj, string name)
    {
        JsonNode? node = obj[name];
        if (node is null)
        {
            throw new WireException("campo mancante: " + name);
        }

        try
        {
            return node.GetValue<bool>();
        }
        catch (Exception ex) when (ex is InvalidOperationException or FormatException)
        {
            throw new WireException("campo " + name + " non è un booleano", ex);
        }
    }

    /// <summary>Legge un numero intero obbligatorio; lancia WireException se assente o malformato.</summary>
    private static long Long(JsonObject obj, string name)
    {
        JsonNode? node = obj[name];
        if (node is null)
        {
            throw new WireException("campo mancante: " + name);
        }

        try
        {
            return node.GetValue<long>();
        }
        catch (Exception ex) when (ex is InvalidOperationException or FormatException)
        {
            throw new WireException("campo " + name + " non è un intero", ex);
        }
    }

    /// <summary>Legge un numero intero opzionale: null se il campo è assente o JSON null, lancia WireException se malformato.</summary>
    private static int? NullableInt(JsonObject obj, string name)
    {
        JsonNode? node = obj[name];
        if (node is null)
        {
            return null;
        }

        try
        {
            return node.GetValue<int?>();
        }
        catch (Exception ex) when (ex is InvalidOperationException or FormatException)
        {
            throw new WireException("campo " + name + " non è un intero", ex);
        }
    }
}
