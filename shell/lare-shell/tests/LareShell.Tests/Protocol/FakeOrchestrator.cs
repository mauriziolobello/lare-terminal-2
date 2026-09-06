using System.Net;
using System.Net.Sockets;
using System.Net.WebSockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;

namespace LareShell.Tests.Protocol;

/// <summary>
/// Orchestratore finto: un server WebSocket minimo in-process. NON usa HttpListener (registra
/// prefissi in http.sys che sopravvivono a un test caduto e richiedono URL ACL): TcpListener su
/// porta 0 + handshake HTTP di upgrade scritto a mano (RFC 6455 §4.2.2: risposta 101 con
/// Sec-WebSocket-Accept = base64(SHA1(key + GUID magico))) + WebSocket.CreateFromStream.
/// Gestisce UN client per volta, come serve ai test.
/// </summary>
internal sealed class FakeOrchestrator : IAsyncDisposable
{
    private const string WebSocketMagicGuid = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

    private readonly TcpListener _listener;
    private readonly string? _expectedToken;
    private TcpClient? _client;
    private WebSocket? _socket;

    private FakeOrchestrator(TcpListener listener, string? expectedToken)
    {
        _listener = listener;
        _expectedToken = expectedToken;
    }

    public int Port => ((IPEndPoint)_listener.LocalEndpoint).Port;

    public Uri Uri => new("ws://127.0.0.1:" + Port + "/");

    /// <param name="expectedToken">null = accetta qualunque token.</param>
    public static FakeOrchestrator Start(string? expectedToken = null)
    {
        var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        return new FakeOrchestrator(listener, expectedToken);
    }

    /// <summary>Accetta un client, fa l'upgrade, legge l'hello. Token sbagliato → chiude senza
    /// server_info (esattamente ciò che fa ws.rs); altrimenti manda server_info. Ritorna l'hello.</summary>
    public async Task<JsonObject> AcceptAsync(CancellationToken ct)
    {
        _client = await _listener.AcceptTcpClientAsync(ct);
        NetworkStream stream = _client.GetStream();
        _socket = await UpgradeAsync(stream, ct);

        JsonObject hello = await ReceiveAsync(ct);
        if (_expectedToken is not null && (string?)hello["token"] != _expectedToken)
        {
            await CloseAsync();
            return hello;
        }

        await SendAsync("{\"type\":\"server_info\",\"version\":\"fake\",\"ai_provider\":\"none\",\"capabilities\":[]}", ct);
        return hello;
    }

    public async Task<JsonObject> ReceiveAsync(CancellationToken ct)
    {
        WebSocket socket = _socket ?? throw new InvalidOperationException("nessun client accettato");
        var buffer = new byte[16 * 1024];
        using var ms = new MemoryStream();
        while (true)
        {
            WebSocketReceiveResult r = await socket.ReceiveAsync(buffer, ct);
            if (r.MessageType == WebSocketMessageType.Close)
            {
                throw new IOException("il client ha chiuso la connessione");
            }

            ms.Write(buffer, 0, r.Count);
            if (r.EndOfMessage)
            {
                break;
            }
        }

        string text = Encoding.UTF8.GetString(ms.ToArray());
        return (JsonObject)JsonNode.Parse(text)!;
    }

    public Task SendAsync(string json, CancellationToken ct)
    {
        WebSocket socket = _socket ?? throw new InvalidOperationException("nessun client accettato");
        return socket.SendAsync(Encoding.UTF8.GetBytes(json), WebSocketMessageType.Text, endOfMessage: true, ct);
    }

    /// <summary>Manda un messaggio di testo spezzato in DUE frame (endOfMessage false, poi true):
    /// serve a verificare che il client riassembli i frame fino a EndOfMessage.</summary>
    public async Task SendFragmentedAsync(string json, int splitAt, CancellationToken ct)
    {
        WebSocket socket = _socket ?? throw new InvalidOperationException("nessun client accettato");
        byte[] bytes = Encoding.UTF8.GetBytes(json);
        await socket.SendAsync(new ArraySegment<byte>(bytes, 0, splitAt), WebSocketMessageType.Text, endOfMessage: false, ct);
        await socket.SendAsync(new ArraySegment<byte>(bytes, splitAt, bytes.Length - splitAt), WebSocketMessageType.Text, endOfMessage: true, ct);
    }

    public async Task CloseAsync()
    {
        if (_socket is { State: WebSocketState.Open or WebSocketState.CloseReceived })
        {
            try
            {
                await _socket.CloseOutputAsync(WebSocketCloseStatus.NormalClosure, "bye", CancellationToken.None);
            }
            catch (Exception ex) when (ex is WebSocketException or IOException or ObjectDisposedException)
            {
                // Il client può aver già chiuso: irrilevante per i test.
            }
        }
    }

    public async ValueTask DisposeAsync()
    {
        await CloseAsync();
        _socket?.Dispose();
        _client?.Dispose();
        _listener.Stop();
    }

    private static async Task<WebSocket> UpgradeAsync(NetworkStream stream, CancellationToken ct)
    {
        // Legge la richiesta HTTP fino alla riga vuota che chiude le intestazioni.
        var buffer = new byte[8 * 1024];
        int total = 0;
        while (true)
        {
            int n = await stream.ReadAsync(buffer.AsMemory(total, buffer.Length - total), ct);
            if (n == 0)
            {
                throw new IOException("handshake troncato");
            }

            total += n;
            if (Encoding.ASCII.GetString(buffer, 0, total).Contains("\r\n\r\n", StringComparison.Ordinal))
            {
                break;
            }

            if (total == buffer.Length)
            {
                throw new IOException("intestazioni HTTP troppo lunghe");
            }
        }

        string request = Encoding.ASCII.GetString(buffer, 0, total);
        string keyLine = request.Split("\r\n").First(l => l.StartsWith("Sec-WebSocket-Key:", StringComparison.OrdinalIgnoreCase));
        string key = keyLine.Split(':', 2)[1].Trim();
        string accept = Convert.ToBase64String(SHA1.HashData(Encoding.ASCII.GetBytes(key + WebSocketMagicGuid)));

        string response = "HTTP/1.1 101 Switching Protocols\r\n" +
                          "Upgrade: websocket\r\n" +
                          "Connection: Upgrade\r\n" +
                          "Sec-WebSocket-Accept: " + accept + "\r\n\r\n";
        byte[] bytes = Encoding.ASCII.GetBytes(response);
        await stream.WriteAsync(bytes, ct);
        await stream.FlushAsync(ct);

        return WebSocket.CreateFromStream(stream, isServer: true, subProtocol: null, keepAliveInterval: TimeSpan.Zero);
    }
}
