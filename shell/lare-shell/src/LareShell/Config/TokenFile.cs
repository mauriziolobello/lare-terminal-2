namespace LareShell.Config;

/// <summary>
/// Il token del WS (spec §8: "WS solo su 127.0.0.1 + token su file") sta in
/// <c>&lt;config-dir&gt;\token</c>, generato dall'orchestratore al primo avvio. Prima che
/// l'orchestratore sia partito il file può non esistere: <c>null</c>, non un'eccezione — il
/// Launcher (Task 6) rilegge il file a ogni tentativo di connessione.
/// </summary>
internal static class TokenFile
{
    public static string? Read(string configDir)
    {
        string path = Path.Combine(configDir, "token");
        try
        {
            if (!File.Exists(path))
            {
                return null;
            }

            string token = File.ReadAllText(path).Trim();
            return token.Length == 0 ? null : token;
        }
        catch (IOException)
        {
            return null;
        }
    }
}
