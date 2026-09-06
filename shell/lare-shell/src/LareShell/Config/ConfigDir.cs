namespace LareShell.Config;

/// <summary>
/// Regola unica di risoluzione della cartella di configurazione (spec §6.1, ADR-017):
/// 1. <c>--config-dir &lt;path&gt;</c> se presente;
/// 2. altrimenti <c>&lt;cartella dell'exe&gt;\..\Configuration\</c> — lare-shell.exe vive in
///    <c>&lt;deploy&gt;\shell\</c>, quindi la Configuration è quella del deploy, accanto a shell\.
/// NESSUNA variabile d'ambiente (LARE_*, LOCALAPPDATA, APPDATA) viene letta: è la regola D6.
/// </summary>
internal static class ConfigDir
{
    public static string Resolve(string? cliValue, string exeDir)
    {
        string dir = string.IsNullOrWhiteSpace(cliValue)
            ? Path.Combine(exeDir, "..", "Configuration")
            : cliValue;
        // GetFullPath normalizza i ".." e la barra finale (C:\Lare\shell\..\Configuration → C:\Lare\Configuration).
        return Path.GetFullPath(dir);
    }

    /// <summary>Radice del deploy = cartella padre di Configuration\ (spec §6.3): lì stanno
    /// orchestrator.exe e ui.exe che la host avvia in autostart (§6.4).</summary>
    public static string DeployRoot(string configDir) => Path.GetFullPath(Path.Combine(configDir, ".."));
}
