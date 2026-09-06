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
        // UnauthorizedAccessException NON deriva da IOException (sono due rami distinti della
        // gerarchia): un permesso negato sul file token va catturato esplicitamente, altrimenti
        // risalirebbe al chiamante invece di degradare a null come previsto dal commento sopra.
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }
}
