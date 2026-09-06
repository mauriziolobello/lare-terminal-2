namespace LareShell.Config;

/// <summary>
/// Argomenti della riga di comando della host (spec §2.3/§6.1):
/// <c>--config-dir &lt;path&gt;</c> (cartella di configurazione), <c>--session &lt;id&gt;</c>
/// (passato da ui.exe in modalità A: lega la connessione shell alla finestra terminale),
/// <c>--selftest</c> (controlli non interattivi, exit code 0/1). Argomenti sconosciuti ignorati.
/// Un <c>record</c> immutabile: è un valore, non un oggetto con comportamento.
/// </summary>
internal sealed record CliArgs(string? ConfigDir, string? SessionId, bool SelfTest)
{
    public static CliArgs Parse(IReadOnlyList<string> args)
    {
        string? configDir = null;
        string? sessionId = null;
        bool selfTest = false;

        for (int i = 0; i < args.Count; i++)
        {
            switch (args[i])
            {
                // `when i + 1 < args.Count`: il flag in coda senza valore viene ignorato,
                // non fa cadere il processo.
                case "--config-dir" when i + 1 < args.Count:
                    configDir = args[++i];
                    break;
                case "--session" when i + 1 < args.Count:
                    sessionId = args[++i];
                    break;
                case "--selftest":
                    selfTest = true;
                    break;
            }
        }

        return new CliArgs(configDir, sessionId, selfTest);
    }
}
