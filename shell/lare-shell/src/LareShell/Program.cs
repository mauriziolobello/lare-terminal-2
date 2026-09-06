using LareShell.Config;

namespace LareShell;

internal static class Program
{
    private static int Main(string[] args)
    {
        // UTF-8 in output: senza, le lettere accentate dei nostri messaggi si corrompono
        // su console con codepage non-UTF8 (lezione dello spike). Try/catch: stdout rediretto.
        try { Console.OutputEncoding = System.Text.Encoding.UTF8; } catch { /* ignorabile */ }

        CliArgs cli = CliArgs.Parse(args);
        string configDir = ConfigDir.Resolve(cli.ConfigDir, AppContext.BaseDirectory);
        Console.WriteLine("lare-shell " + HostInfo.Version + " — config: " + configDir);
        return 0;
    }
}
