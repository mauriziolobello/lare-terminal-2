using System.Net.Sockets;
using System.Net.WebSockets;
using System.Text;
using System.Threading.Channels;
using LareShell.Config;

namespace LareShell.Protocol;

/// <summary>
/// Connessione WS persistente verso l'orchestratore (spec §4): handshake <c>hello</c>
/// (role shell, session_id, cwd, version) → <c>server_info</c>; poi un loop di ricezione su un
/// thread del pool che parsa ogni messaggio e lo ACCODA in un <c>Channel</c>. È l'unico ruolo del
/// thread del socket: non tocca mai console né runspace (§4.4 — "il thread WS accoda; il REPL
/// consuma"). Quando la connessione cade, accoda un <c>Disconnected</c> sintetico.
///
/// Nessuna riconnessione automatica in background: chi usa il client (Repl/Launcher) richiama
/// <c>Connect</c> quando serve (ruling 2 del piano). <c>tokenProvider</c> viene invocato a ogni
/// connessione: il file <c>token</c> può non esistere ancora al primo tentativo (l'orchestratore
/// lo crea all'avvio).
///
/// Regola per tutto il file: ogni <c>await</c> porta <c>.ConfigureAwait(false)</c>. Il REPL chiama
/// i ponti sincroni <c>Connect</c>/<c>Send</c> (<c>.GetAwaiter().GetResult()</c>): se una
/// continuazione cercasse di tornare su un SynchronizationContext del thread chiamante mentre quel
/// thread è bloccato in attesa, sarebbe un deadlock. Con <c>ConfigureAwait(false)</c> le
/// continuazioni restano sul pool.
/// </summary>
internal sealed class OrchestratorClient : IDisposable
{
    private static readonly TimeSpan SyncTimeout = TimeSpan.FromSeconds(5);

    private readonly Uri _uri;
    private readonly Func<string?> _tokenProvider;
    private readonly string _version;
    private readonly HostLog _log;
    private readonly Channel<ServerMessage> _incoming = Channel.CreateUnbounded<ServerMessage>(
        new UnboundedChannelOptions { SingleReader = true, SingleWriter = true });
    private readonly SemaphoreSlim _sendLock = new(1, 1);

    private ClientWebSocket? _socket;
    private Task? _receiveLoop;

    public OrchestratorClient(Uri uri, Func<string?> tokenProvider, string sessionId, string version, HostLog log)
    {
        _uri = uri;
        _tokenProvider = tokenProvider;
        SessionId = sessionId;
        _version = version;
        _log = log;
    }

    public string SessionId { get; }

    /// <summary>Il lato di lettura del canale: lo consuma SOLO il thread del REPL.</summary>
    public ChannelReader<ServerMessage> Incoming => _incoming.Reader;

    public bool IsConnected => _socket is { State: WebSocketState.Open };

    /// <summary>Connette e fa l'handshake. Ritorna <c>null</c> se connesso, altrimenti un motivo
    /// leggibile (da stampare nel terminale). Non lancia per gli errori di rete attesi.</summary>
    public async Task<string?> ConnectAsync(string cwd, CancellationToken ct)
    {
        string? token = _tokenProvider();
        if (string.IsNullOrEmpty(token))
        {
            return "token non trovato (l'orchestratore lo crea al primo avvio)";
        }

        await ShutdownSocketAsync().ConfigureAwait(false);

        var socket = new ClientWebSocket();
        try
        {
            await socket.ConnectAsync(_uri, ct).ConfigureAwait(false);
            await SendRawAsync(socket, Wire.Hello(token, SessionId, cwd, _version), ct).ConfigureAwait(false);

            string? first = await ReceiveTextAsync(socket, ct).ConfigureAwait(false);
            if (first is null)
            {
                socket.Dispose();
                return "token rifiutato o connessione chiusa durante l'handshake";
            }

            if (Wire.Parse(first) is not ServerInfo)
            {
                socket.Dispose();
                return "handshake inatteso: " + first;
            }
        }
        catch (Exception ex) when (ex is WebSocketException or SocketException or IOException or HttpRequestException or WireException)
        {
            socket.Dispose();
            return "connessione fallita: " + ex.Message;
        }

        _socket = socket;
        _receiveLoop = Task.Run(() => ReceiveLoopAsync(socket));
        _log.Info("connesso a " + _uri + " (sessione " + SessionId + ")");
        return null;
    }

    /// <summary>Ponte sincrono per il thread del REPL (che non è async: vedi Global Constraints).</summary>
    public string? Connect(string cwd, TimeSpan timeout)
    {
        using var cts = new CancellationTokenSource(timeout);
        try
        {
            return ConnectAsync(cwd, cts.Token).GetAwaiter().GetResult();
        }
        catch (OperationCanceledException)
        {
            return "timeout di connessione (" + timeout.TotalSeconds + " s)";
        }
    }

    /// <summary>Invia un messaggio. <c>false</c> se non connessi o se l'invio fallisce (mai un'eccezione).</summary>
    public async Task<bool> SendAsync(string json, CancellationToken ct)
    {
        ClientWebSocket? socket = _socket;
        if (socket is not { State: WebSocketState.Open })
        {
            return false;
        }

        try
        {
            // Un solo SendAsync alla volta per socket: è un vincolo di ClientWebSocket.
            await _sendLock.WaitAsync(ct).ConfigureAwait(false);
            try
            {
                await SendRawAsync(socket, json, ct).ConfigureAwait(false);
            }
            finally
            {
                _sendLock.Release();
            }

            return true;
        }
        catch (Exception ex) when (ex is WebSocketException or IOException or ObjectDisposedException or OperationCanceledException)
        {
            _log.Warn("invio fallito: " + ex.Message);
            return false;
        }
    }

    public bool Send(string json)
    {
        using var cts = new CancellationTokenSource(SyncTimeout);
        return SendAsync(json, cts.Token).GetAwaiter().GetResult();
    }

    public void Dispose()
    {
        ShutdownSocketAsync().GetAwaiter().GetResult();
        _sendLock.Dispose();
    }

    private async Task ReceiveLoopAsync(ClientWebSocket socket)
    {
        string reason;
        try
        {
            while (true)
            {
                string? text = await ReceiveTextAsync(socket, CancellationToken.None).ConfigureAwait(false);
                if (text is null)
                {
                    reason = "chiusa dall'orchestratore";
                    break;
                }

                ServerMessage message;
                try
                {
                    message = Wire.Parse(text);
                }
                catch (WireException ex)
                {
                    _log.Warn("messaggio scartato: " + ex.Message);
                    continue;
                }

                _incoming.Writer.TryWrite(message);
            }
        }
        catch (Exception ex) when (ex is WebSocketException or IOException or ObjectDisposedException or OperationCanceledException)
        {
            reason = ex.Message;
        }

        _log.Info("connessione chiusa: " + reason);
        _incoming.Writer.TryWrite(new Disconnected(reason));
    }

    /// <summary>Legge UN messaggio di testo completo, riassemblando i frame fino a EndOfMessage
    /// (un messaggio lungo arriva spezzato). <c>null</c> = frame di chiusura.</summary>
    private static async Task<string?> ReceiveTextAsync(WebSocket socket, CancellationToken ct)
    {
        var buffer = new byte[16 * 1024];
        using var ms = new MemoryStream();
        while (true)
        {
            WebSocketReceiveResult r = await socket.ReceiveAsync(buffer, ct).ConfigureAwait(false);
            if (r.MessageType == WebSocketMessageType.Close)
            {
                return null;
            }

            ms.Write(buffer, 0, r.Count);
            if (r.EndOfMessage)
            {
                return Encoding.UTF8.GetString(ms.ToArray());
            }
        }
    }

    private static Task SendRawAsync(WebSocket socket, string json, CancellationToken ct) =>
        socket.SendAsync(Encoding.UTF8.GetBytes(json), WebSocketMessageType.Text, endOfMessage: true, ct);

    private async Task ShutdownSocketAsync()
    {
        ClientWebSocket? old = _socket;
        _socket = null;
        if (old is null)
        {
            return;
        }

        // Abort (non Close): non vogliamo aspettare un server che magari è morto. Il loop di
        // ricezione esce con un'eccezione e accoda Disconnected; lo attendiamo per non lasciare task in volo.
        try { old.Abort(); } catch { /* già chiuso */ }
        old.Dispose();
        if (_receiveLoop is not null)
        {
            try { await _receiveLoop.ConfigureAwait(false); } catch { /* le eccezioni del loop sono già gestite dentro */ }
            _receiveLoop = null;
        }
    }
}
