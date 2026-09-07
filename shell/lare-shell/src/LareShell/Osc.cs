namespace LareShell;

/// <summary>
/// Canale diretto host → emulatore (spec §4.7): la sequenza OSC 9001 che segnala l'ultimo
/// comando intercettato. In modalità B Windows Terminal la ignora (OSC sconosciuta); in modalità A
/// (piano 3) xterm.js la cattura con registerOscHandler(9001, …). ESC è costruito da intero:
/// MAI con un escape di stringa (backslash, x, 1, b) in C# — l'escape \x è "goloso" e mangia le
/// cifre esadecimali che seguono (bug dello spike 1); SourceScanTests fallisce se ricompare.
/// </summary>
internal static class Osc
{
    internal static readonly char Esc = (char)0x1B;

    public static string Intercept(string line) =>
        Esc + "]9001;lare;intercept;" + Sanitize(line) + Esc + "\\";

    /// <summary>Un ESC, un BEL o un a-capo nel payload chiuderebbero/spezzerebbero la sequenza.</summary>
    private static string Sanitize(string s) => new(s.Where(c => !char.IsControl(c)).ToArray());
}
