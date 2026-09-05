using System.Runtime.InteropServices;

namespace LareShellSpike;

/// <summary>
/// ConsoleModes centralizza le uniche P/Invoke Win32 che servono a questo spike per
/// leggere/scrivere la "console mode" (GetConsoleMode/SetConsoleMode) degli handle
/// standard di output e di input.
///
/// Prima di questo refactor le stesse tre dichiarazioni [DllImport] vivevano solo
/// dentro StatusBar (che le usa per accendere ENABLE_VIRTUAL_TERMINAL_PROCESSING
/// sull'output). Ora servono anche a LareHost per implementare
/// NotifyBeginApplication/NotifyEndApplication (vedi LareHost.cs): un'unica classe
/// statica evita la duplicazione delle dichiarazioni [DllImport].
///
/// È SOLO Windows: kernel32.dll non esiste su altre piattaforme. Chi chiama questa
/// classe deve controllare OperatingSystem.IsWindows() PRIMA di usarla (qui non lo
/// ricontrolliamo per restare una classe "dumb" di solo P/Invoke).
/// </summary>
internal static class ConsoleModes
{
    // Valori "well-known" passati a GetStdHandle: sono costanti Win32 standard per
    // riferirsi agli handle di I/O standard del processo, non handle veri e propri.
    public const int StdOutputHandle = -11;
    public const int StdInputHandle = -10;

    // Sottoinsieme dei flag di dwMode che ci servono (vedi documentazione Win32
    // "Console Mode" per l'elenco completo).
    public const uint EnableProcessedOutput = 0x0001;
    public const uint EnableVirtualTerminalProcessing = 0x0004;

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetStdHandle(int nStdHandle);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetConsoleMode(IntPtr hConsoleHandle, out uint lpMode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetConsoleMode(IntPtr hConsoleHandle, uint dwMode);

    /// <summary>Ottiene l'handle Win32 per uno degli standard device (vedi le costanti sopra).</summary>
    public static IntPtr GetHandle(int stdHandle) => GetStdHandle(stdHandle);

    /// <summary>Legge la mode corrente di un handle console. False se la chiamata Win32 fallisce
    /// (handle non valido, o non è davvero un handle di console: capita quando stdout/stdin
    /// sono rediretti verso un file o una pipe, come in --selftest).</summary>
    public static bool TryGetMode(IntPtr handle, out uint mode)
    {
        mode = 0;
        if (handle == IntPtr.Zero || handle == new IntPtr(-1))
        {
            return false;
        }

        return GetConsoleMode(handle, out mode);
    }

    /// <summary>Imposta la mode di un handle console. False se la chiamata Win32 fallisce.</summary>
    public static bool TrySetMode(IntPtr handle, uint mode)
    {
        if (handle == IntPtr.Zero || handle == new IntPtr(-1))
        {
            return false;
        }

        return SetConsoleMode(handle, mode);
    }

    /// <summary>
    /// Abilita ENABLE_VIRTUAL_TERMINAL_PROCESSING sull'handle di output standard, cioè
    /// dice a Windows di interpretare le sequenze ANSI/VT100 che scriviamo (colori,
    /// cursore, OSC, ecc.) invece di mostrarle come testo letterale.
    ///
    /// STORIA (spike 2, Tauri): originariamente questa logica viveva SOLO dentro il
    /// costruttore di StatusBar, quindi veniva attivata solo in modalità interattiva
    /// "con barre". Con l'introduzione di `--no-bars` (la StatusBar non viene più
    /// creata quando la finestra Tauri fornisce già la sua chrome HTML) serviva
    /// comunque un modo per abilitare la VT processing anche SENZA StatusBar, perché
    /// l'OSC 9001 che segnala le righe "/" intercettate (vedi Repl.TryIntercept) va
    /// scritta correttamente in ogni caso. Spostata qui (classe già dedicata alle
    /// primitive Win32 di console mode) ed esposta come metodo pubblico, così sia
    /// StatusBar sia Repl.RunInteractive possono chiamarla senza duplicare codice.
    /// </summary>
    public static bool TryEnableVirtualTerminalProcessing()
    {
        if (!OperatingSystem.IsWindows())
        {
            // Su Linux/macOS i terminali moderni interpretano già le sequenze VT
            // di default: non serve alcuna chiamata P/Invoke (che comunque non
            // esisterebbe, essendo kernel32 Windows-only).
            return true;
        }

        try
        {
            IntPtr handle = GetHandle(StdOutputHandle);
            if (!TryGetMode(handle, out uint mode))
            {
                return false;
            }

            mode |= EnableVirtualTerminalProcessing | EnableProcessedOutput;
            return TrySetMode(handle, mode);
        }
        catch
        {
            return false;
        }
    }
}
