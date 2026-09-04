using System.Runtime.InteropServices;
using System.Text;

namespace LareShellSpike;

/// <summary>
/// StatusBar disegna due righe "fisse" nel terminale usando sequenze VT100/ANSI:
///  - riga 1 (in alto): indicatori (nome shell, spinner, orologio, ultimo comando
///    intercettato);
///  - ultima riga (in basso): una barra di stato in "inverse video" con qualche
///    voce di menu, una delle quali è un vero hyperlink OSC 8.
///
/// La tecnica chiave è DECSTBM ("Set Top and Bottom Margins", ESC [ top ; bottom r):
/// dice al terminale "la regione di scroll va dalla riga `top` alla riga `bottom`".
/// Tutto quello che scrolla (l'output dei comandi, incluso quello scritto da
/// PSReadLine) resta confinato in quella regione, mentre le righe 1 e H (fuori dai
/// margini) restano ferme: è così che pwsh "vero" non fa nulla del genere, ma
/// strumenti come tmux/vim disegnano barre di stato fisse.
///
/// QUESTA È DELIBERATAMENTE LA PARTE PIÙ RISCHIOSA DELLO SPIKE: un thread di
/// background ridisegna la riga 1 ogni secondo scrivendo sulla stessa Console su
/// cui, in parallelo, PSReadLine sta disegnando la riga di editing dell'utente.
/// Nessuna delle due parti sa dell'esistenza dell'altra. Il lock condiviso con
/// Repl (vedi Repl.ConsoleLock) riduce il rischio di interleaving a metà di una
/// singola Console.Write, ma non elimina la possibilità che PSReadLine, che tiene
/// un proprio stato interno di "dove pensa che sia il cursore", si confonda dopo
/// che noi lo spostiamo (save/restore cursore) per disegnare le barre. Va osservato
/// e riportato, non è garantito che sia perfetto: è proprio quello che lo spike
/// deve verificare.
/// </summary>
internal sealed class StatusBar
{
    private const string LibraryUrl = "https://github.com/mauriziolobello/lare-terminal-2";

    private readonly object _consoleLock;
    private readonly bool _enabled;
    private readonly string[] _spinnerFrames = { "|", "/", "-", "\\" };

    private Thread? _thread;
    private volatile bool _running;
    private volatile string _lastCommand = "(nessuno)";
    private int _spinnerIndex;
    private int _lastWidth;
    private int _lastHeight;

    public StatusBar(object consoleLock)
    {
        _consoleLock = consoleLock;

        // La status bar ha senso solo con una console interattiva reale: se lo
        // stdout è rediretto (pipe, file, o modalità --selftest) non proviamo
        // nemmeno ad abilitare la VT processing o a disegnare le barre.
        _enabled = !Console.IsOutputRedirected && !Console.IsErrorRedirected && EnableVirtualTerminalProcessing();
    }

    /// <summary>True se la console supporta le sequenze VT e non è rediretta.</summary>
    public bool Enabled => _enabled;

    /// <summary>Aggiorna il testo dell'ultimo comando intercettato mostrato in alto a destra.</summary>
    public void SetLastCommand(string command) => _lastCommand = command;

    public void Start()
    {
        if (!_enabled)
        {
            return;
        }

        _lastWidth = SafeWidth();
        _lastHeight = SafeHeight();

        lock (_consoleLock)
        {
            ApplyScrollRegion(_lastHeight);
            DrawTopRow();
            DrawBottomRow();
        }

        _running = true;
        _thread = new Thread(Loop)
        {
            IsBackground = true, // non deve impedire la chiusura del processo
            Name = "LareStatusBar",
        };
        _thread.Start();
    }

    public void Stop()
    {
        if (!_enabled)
        {
            return;
        }

        _running = false;
        _thread?.Join(TimeSpan.FromMilliseconds(500));

        lock (_consoleLock)
        {
            // ESC [ r senza parametri = rimuove i margini di scroll (torna a usare
            // tutta l'altezza del buffer). Poi puliamo le due righe che avevamo
            // riservato e ci assicuriamo che il cursore sia visibile.
            var sb = new StringBuilder();
            sb.Append("\x1b[r");
            sb.Append("\x1b7");
            sb.Append("\x1b[1;1H\x1b[2K");
            sb.Append("\x1b[").Append(_lastHeight).Append(";1H\x1b[2K");
            sb.Append("\x1b8");
            sb.Append("\x1b[?25h"); // mostra il cursore, per sicurezza
            Console.Out.Write(sb.ToString());
            Console.Out.Flush();
        }
    }

    private void Loop()
    {
        // Poll leggero: ogni 250ms controlliamo se la finestra è stata ridimensionata
        // (Console non offre un evento di resize su .NET multipiattaforma, solo
        // polling); ogni ~1s (4 iterazioni) avanziamo lo spinner e l'orologio.
        int ticksSinceClock = 0;
        while (_running)
        {
            Thread.Sleep(250);
            if (!_running)
            {
                break;
            }

            int width = SafeWidth();
            int height = SafeHeight();
            if (width != _lastWidth || height != _lastHeight)
            {
                _lastWidth = width;
                _lastHeight = height;
                lock (_consoleLock)
                {
                    ApplyScrollRegion(height);
                    DrawTopRow();
                    DrawBottomRow();
                }
                ticksSinceClock = 0;
                continue;
            }

            ticksSinceClock++;
            if (ticksSinceClock >= 4)
            {
                ticksSinceClock = 0;
                _spinnerIndex = (_spinnerIndex + 1) % _spinnerFrames.Length;
                lock (_consoleLock)
                {
                    DrawTopRow();
                }
            }
        }
    }

    /// <summary>
    /// Imposta la regione di scroll DECSTBM fra la riga 2 e la penultima riga
    /// (H-1), lasciando quindi la riga 1 e la riga H fuori dallo scroll.
    ///
    /// Nota delicata: il terminale, quando applica DECSTBM, riporta il cursore
    /// all'origine (1,1) della viewport. Se lo facessimo senza precauzioni,
    /// sposteremmo il cursore da sotto le mani dell'utente (o di PSReadLine, se
    /// sta disegnando la riga di editing) ogni volta che ridimensiona la finestra.
    /// Per questo: (1) impostiamo i margini dentro un save/restore cursore
    /// (ESC 7 / ESC 8), così la posizione "logica" del cursore non cambia agli
    /// occhi del chiamante; (2) solo SE la posizione corrente risulta fuori dalla
    /// nuova regione (es. era sulla riga 1 o sull'ultima riga, che ora sono
    /// riservate) la clampiamo dentro i margini. In caso contrario lasciamo il
    /// cursore esattamente dov'era.
    /// </summary>
    private static void ApplyScrollRegion(int height)
    {
        int bottom = Math.Max(3, height - 1);

        // Console.CursorTop è relativo al BUFFER (che può essere più alto della
        // finestra visibile), mentre le sequenze VT che scriviamo sono relative
        // alla VIEWPORT: sottraiamo Console.WindowTop per convertire fra i due
        // sistemi di riferimento. Avvolto in try/catch: se la console non è
        // disponibile (rediretta) semplicemente non proviamo a clampare nulla.
        try
        {
            int currentRow = Console.CursorTop - Console.WindowTop + 1; // 1-based, viewport-relative

            Console.Out.Write("\x1b7\x1b[2;" + bottom + "r\x1b8");

            if (currentRow < 2)
            {
                Console.Out.Write("\x1b[2;1H");
            }
            else if (currentRow > bottom)
            {
                Console.Out.Write("\x1b[" + bottom + ";1H");
            }
            // altrimenti: il cursore era già dentro la regione, non lo tocchiamo.
        }
        catch
        {
            // Console non disponibile: impostiamo comunque i margini (utile in
            // --selftest, dove comunque la StatusBar è disabilitata a monte, ma
            // per sicurezza restiamo difensivi) senza tentare di leggere/spostare
            // il cursore.
            Console.Out.Write("\x1b[2;" + bottom + "r");
        }
    }

    private void DrawTopRow()
    {
        int width = SafeWidth();
        string left = " Lare Terminal (spike)";
        string right = _spinnerFrames[_spinnerIndex] + " " + DateTime.Now.ToString("HH:mm:ss") + "  ultimo: " + _lastCommand + " ";

        int gap = Math.Max(1, (width - 1) - left.Length - right.Length);
        string content = left + new string(' ', gap) + right;
        content = Truncate(content, width - 1);

        var sb = new StringBuilder();
        sb.Append("\x1b7");           // salva posizione cursore (ESC 7)
        sb.Append("\x1b[1;1H");        // vai alla riga 1, colonna 1
        sb.Append("\x1b[2K");          // pulisci l'intera riga
        sb.Append("\x1b[7m").Append(content).Append("\x1b[0m"); // inverse video
        sb.Append("\x1b8");           // ripristina posizione cursore (ESC 8)

        Console.Out.Write(sb.ToString());
        Console.Out.Flush();
    }

    private void DrawBottomRow()
    {
        int width = SafeWidth();
        int row = SafeHeight();

        const string helpText = " /help  ";
        const string libraryVisible = "/library";
        // OSC 8 è la sequenza standard per gli hyperlink nel terminale:
        // ESC ] 8 ; ; URL ESC \  testo-visibile  ESC ] 8 ; ; ESC \
        // Il terminale (se lo supporta, es. Windows Terminal) rende "testo-visibile"
        // cliccabile e apre URL. I byte della sequenza non occupano colonne visibili,
        // per questo calcoliamo la larghezza "visibile" separatamente (solo
        // helpText + libraryVisible + restText), non la lunghezza della stringa
        // finale che include anche i byte di escape.
        string libraryLink = "\x1b]8;;" + LibraryUrl + "\x1b\\" + libraryVisible + "\x1b]8;;\x1b\\";
        const string restText = "  /aichat  /config ";

        int visibleLength = helpText.Length + libraryVisible.Length + restText.Length;
        int pad = Math.Max(0, (width - 1) - visibleLength);
        string body = helpText + libraryLink + restText + new string(' ', pad);

        var sb = new StringBuilder();
        sb.Append("\x1b7");
        sb.Append("\x1b[").Append(row).Append(";1H");
        sb.Append("\x1b[2K");
        sb.Append("\x1b[7m").Append(body).Append("\x1b[0m");
        sb.Append("\x1b8");

        Console.Out.Write(sb.ToString());
        Console.Out.Flush();
    }

    private static string Truncate(string s, int maxVisibleLength)
    {
        if (maxVisibleLength <= 0) return string.Empty;
        return s.Length <= maxVisibleLength ? s : s[..maxVisibleLength];
    }

    private static int SafeWidth()
    {
        try { return Math.Max(20, Console.WindowWidth); }
        catch { return 120; }
    }

    private static int SafeHeight()
    {
        try { return Math.Max(5, Console.WindowHeight); }
        catch { return 30; }
    }

    // --- P/Invoke: abilitazione della VT processing su Windows ---------------
    // .NET su Windows non abilita di default ENABLE_VIRTUAL_TERMINAL_PROCESSING
    // sull'handle di output standard quando si scrive tramite System.Console in
    // un'app console "vecchio stile": bisogna chiedere esplicitamente a Win32 di
    // interpretare le sequenze ANSI/VT100 che scriviamo, altrimenti verrebbero
    // mostrate come testo letterale (pieno di ESC[...) invece che come comandi.

    private const int StdOutputHandle = -11;
    private const uint EnableProcessedOutput = 0x0001;
    private const uint EnableVirtualTerminalProcessingFlag = 0x0004;

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetStdHandle(int nStdHandle);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetConsoleMode(IntPtr hConsoleHandle, out uint lpMode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetConsoleMode(IntPtr hConsoleHandle, uint dwMode);

    private static bool EnableVirtualTerminalProcessing()
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
            IntPtr handle = GetStdHandle(StdOutputHandle);
            if (handle == IntPtr.Zero || handle == new IntPtr(-1))
            {
                return false;
            }

            if (!GetConsoleMode(handle, out uint mode))
            {
                return false;
            }

            mode |= EnableVirtualTerminalProcessingFlag | EnableProcessedOutput;
            return SetConsoleMode(handle, mode);
        }
        catch
        {
            return false;
        }
    }
}
