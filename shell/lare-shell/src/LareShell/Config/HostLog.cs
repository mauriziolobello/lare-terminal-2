namespace LareShell.Config;

/// <summary>
/// Log su file della host: <c>&lt;config-dir&gt;\logs\lare-shell.log</c> (spec §6.2). Best-effort:
/// se la cartella non si può creare o il file non si può scrivere, ogni chiamata è un no-op —
/// un problema di log non deve mai far cadere la shell (§9). Senza rotazione (debito, HANDOFF).
/// Thread-safe (lock): scrivono sia il thread del REPL sia il loop di ricezione del WS.
/// </summary>
internal sealed class HostLog
{
    private readonly string? _path;
    private readonly object _lock = new();

    private HostLog(string? path) => _path = path;

    /// <summary>Log che scarta tutto (test, o quando la cartella non è scrivibile).</summary>
    public static HostLog Null { get; } = new(null);

    public static HostLog Open(string configDir)
    {
        try
        {
            string dir = Path.Combine(configDir, "logs");
            Directory.CreateDirectory(dir);
            return new HostLog(Path.Combine(dir, "lare-shell.log"));
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            return Null;
        }
    }

    public void Info(string message) => Write("INFO", message);

    public void Warn(string message) => Write("WARN", message);

    public void Debug(string message) => Write("DEBUG", message);

    private void Write(string level, string message)
    {
        if (_path is null)
        {
            return;
        }

        lock (_lock)
        {
            try
            {
                File.AppendAllText(_path, DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss") + " " + level + " " + message + Environment.NewLine);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // Best-effort: vedi il commento di classe.
            }
        }
    }
}
