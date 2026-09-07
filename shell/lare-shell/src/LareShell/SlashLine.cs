namespace LareShell;

/// <summary>
/// Riconoscimento delle righe da intercettare (spec §1.1: "i comandi slash vengono interpretati
/// dal nostro programma"): una riga la cui prima cosa non-spazio è '/'. Il resto del routing
/// (comando noto/ignoto, /ai con virgolette…) lo fa l'orchestratore, non la host.
/// Funzioni pure: la stessa logica del REPL, testata senza console.
/// </summary>
internal static class SlashLine
{
    public static bool IsSlash(string raw) => raw.TrimStart().StartsWith('/');

    /// <summary>Il testo inviato come <c>Command.input</c>: solo trim ai bordi.</summary>
    public static string Normalize(string raw) => raw.Trim();
}
