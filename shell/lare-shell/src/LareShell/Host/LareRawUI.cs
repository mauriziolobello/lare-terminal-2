using System.Management.Automation.Host;

namespace LareShell.Host;

/// <summary>
/// LareRawUI implementa PSHostRawUserInterface: è la parte "a basso livello" della UI,
/// quella con cui il motore (e soprattutto PSReadLine) manipola direttamente il buffer
/// della console: colori correnti, posizione del cursore, dimensioni finestra/buffer,
/// lettura di singoli tasti (ReadKey) usata per l'editing di riga.
///
/// Qui ci limitiamo a "tradurre" ogni membro sulle proprietà equivalenti di
/// System.Console. Ogni accesso a Console è avvolto in try/catch perché, se lo
/// stdout/stdin è rediretto (es. `dotnet run -- --selftest` oppure una pipe),
/// molte proprietà di Console lanciano eccezioni invece di restituire un default:
/// non vogliamo che l'host crashi solo perché non c'è una console "vera".
/// </summary>
internal sealed class LareRawUI : PSHostRawUserInterface
{
    // Valori di fallback usati quando System.Console non è disponibile
    // (stdout/stdin rediretti, nessuna console allocata, ecc.).
    private const int FallbackWidth = 120;
    private const int FallbackHeight = 30;

    public override ConsoleColor ForegroundColor
    {
        get => TryGet(() => Console.ForegroundColor, ConsoleColor.Gray);
        set => TrySet(() => Console.ForegroundColor = value);
    }

    public override ConsoleColor BackgroundColor
    {
        get => TryGet(() => Console.BackgroundColor, ConsoleColor.Black);
        set => TrySet(() => Console.BackgroundColor = value);
    }

    public override Coordinates CursorPosition
    {
        get => TryGet(() => new Coordinates(Console.CursorLeft, Console.CursorTop), new Coordinates(0, 0));
        set => TrySet(() => Console.SetCursorPosition(value.X, value.Y));
    }

    public override Coordinates WindowPosition
    {
        get => TryGet(() => new Coordinates(Console.WindowLeft, Console.WindowTop), new Coordinates(0, 0));
        set => TrySet(() => Console.SetWindowPosition(value.X, value.Y));
    }

    public override int CursorSize
    {
        // Console.CursorSize è supportato solo su Windows; su altre piattaforme lancia
        // PlatformNotSupportedException, per questo il fallback try/catch è necessario.
        get => TryGet(() => Console.CursorSize, 25);
        set => TrySet(() => Console.CursorSize = value);
    }

    public override Size WindowSize
    {
        get => TryGet(() => new Size(Console.WindowWidth, Console.WindowHeight), new Size(FallbackWidth, FallbackHeight));
        set => TrySet(() => Console.SetWindowSize(value.Width, value.Height));
    }

    public override Size BufferSize
    {
        get => TryGet(() => new Size(Console.BufferWidth, Console.BufferHeight), new Size(FallbackWidth, FallbackHeight));
        set => TrySet(() => Console.SetBufferSize(value.Width, value.Height));
    }

    public override Size MaxWindowSize =>
        TryGet(() => new Size(Console.BufferWidth, Console.BufferHeight), new Size(FallbackWidth, FallbackHeight));

    public override Size MaxPhysicalWindowSize =>
        TryGet(() => new Size(Console.LargestWindowWidth, Console.LargestWindowHeight), new Size(FallbackWidth, FallbackHeight));

    public override string WindowTitle
    {
        get => TryGet(() => Console.Title, HostInfo.Name);
        set => TrySet(() => Console.Title = value);
    }

    public override bool KeyAvailable => TryGet(() => Console.KeyAvailable, false);

    /// <summary>
    /// Usato dal motore (e in parte da PSReadLine, anche se PSReadLine legge quasi
    /// sempre direttamente da Console.ReadKey per l'editing) per leggere un singolo
    /// tasto. Mappiamo le ReadKeyOptions sul parametro intercept di Console.ReadKey.
    /// </summary>
    public override KeyInfo ReadKey(ReadKeyOptions options)
    {
        // Console.ReadKey(intercept: true) non stampa il tasto premuto: lo facciamo
        // sempre così perché è compito del chiamante (PSReadLine o i cmdlet di prompt)
        // decidere se e come visualizzarlo.
        ConsoleKeyInfo key = Console.ReadKey(intercept: true);

        char c = key.KeyChar;
        ControlKeyStates states = 0;
        if ((key.Modifiers & ConsoleModifiers.Alt) != 0) states |= ControlKeyStates.LeftAltPressed;
        if ((key.Modifiers & ConsoleModifiers.Control) != 0) states |= ControlKeyStates.LeftCtrlPressed;
        if ((key.Modifiers & ConsoleModifiers.Shift) != 0) states |= ControlKeyStates.ShiftPressed;

        return new KeyInfo((int)key.Key, c, states, keyDown: true);
    }

    public override void FlushInputBuffer()
    {
        try
        {
            while (Console.KeyAvailable)
            {
                Console.ReadKey(intercept: true);
            }
        }
        catch
        {
            // Nessun input disponibile da svuotare (console rediretta): ignoriamo.
        }
    }

    // --- Manipolazione diretta del buffer schermo ---------------------------
    // Questi tre membri servirebbero per leggere/scrivere/scrollare rettangoli di
    // celle del buffer console (usati ad es. da Clear-Host per "spazzare" lo schermo,
    // o da alcuni moduli per disegnare popup). Implementarli con le API Win32 reali
    // (ReadConsoleOutput/WriteConsoleOutput) è possibile ma fuori MVP: nessuna barra VT
    // nella 2.0 (D13: le barre sono HTML nella finestra, non righe disegnate nel buffer
    // console). Per non far esplodere l'host quando lo script chiama `cls`
    // (che internamente chiama proprio SetBufferContents), trattiamo il caso speciale
    // "rettangolo = tutto il buffer" richiamando semplicemente Console.Clear().
    public override BufferCell[,] GetBufferContents(Rectangle rectangle) =>
        throw new NotImplementedException("GetBufferContents non è implementato in questa host (fuori MVP).");

    public override void SetBufferContents(Rectangle rectangle, BufferCell fill)
    {
        // Clear-Host chiama SetBufferContents con un Rectangle "tutto -1"
        // (cioè "l'intero buffer") per pulire lo schermo. È l'unico caso che
        // gestiamo esplicitamente; per ogni altro rettangolo dichiariamo la
        // funzionalità non implementata.
        bool isWholeBuffer = rectangle.Left == -1 && rectangle.Top == -1 &&
                              rectangle.Right == -1 && rectangle.Bottom == -1;
        if (isWholeBuffer)
        {
            TrySet(() => Console.Clear());

            // Nella 2.0 le barre sono finestre HTML fuori dall'area terminale (D13),
            // non righe VT disegnate dentro il buffer console: dopo Console.Clear()
            // non c'è quindi nulla da ridisegnare qui.
            return;
        }

        throw new NotImplementedException(
            "SetBufferContents su un rettangolo arbitrario non è implementato in questa host (fuori MVP, solo il caso Clear-Host).");
    }

    public override void SetBufferContents(Coordinates origin, BufferCell[,] contents) =>
        throw new NotImplementedException("SetBufferContents(origin, contents) non è implementato in questa host (fuori MVP).");

    public override void ScrollBufferContents(Rectangle source, Coordinates destination, Rectangle clip, BufferCell fill) =>
        throw new NotImplementedException("ScrollBufferContents non è implementato in questa host (fuori MVP).");

    // --- Helper privati per l'accesso "difensivo" a System.Console ----------

    private static T TryGet<T>(Func<T> getter, T fallback)
    {
        try { return getter(); }
        catch { return fallback; }
    }

    private static void TrySet(Action setter)
    {
        try { setter(); }
        catch
        {
            // Console non disponibile o non supporta l'operazione (es. stdout
            // rediretto): ignoriamo silenziosamente, coerentemente con lo scopo
            // didattico di questa classe.
        }
    }
}
