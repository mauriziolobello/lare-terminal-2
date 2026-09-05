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
///
/// ATTENZIONE, LEZIONE APPRESA (round 2 dello spike, bug confermato): in C# l'escape
/// di stringa "\x" è "greedy" fino a 4 cifre esadecimali. Il letterale scritto come
/// backslash-x-1-b-7 NON è "ESC" seguito dal carattere '7': il compilatore legge
/// "1b7" come un unico numero esadecimale (0x1B7 = U+01B7, la lettera "Ezh"), e la
/// stessa cosa scritta con una '8' finale diventa U+01B8 (Ezh maiuscola). Sintomo
/// osservato: quei glifi comparivano letteralmente a schermo (invece di eseguire
/// DECSC/DECRC, salva/ripristina cursore) e, non essendo mai state eseguite le
/// sequenze di ripristino del cursore, la barra in basso restava "storta" dopo ogni
/// ridisegno, il prompt finiva sopra la barra, eccetera. L'alternativa "corretta"
/// sarebbe l'escape Unicode a ESATTAMENTE 4 cifre esadecimali (che non è "greedy":
/// si ferma sempre dopo 4 cifre) — ma anche quella resta un dettaglio sintattico
/// facile da scrivere o rileggere male. Per eliminare il problema alla radice, qui
/// sotto il carattere ESC non è MAI scritto come escape di stringa: è costruito una
/// sola volta con un cast esplicito da intero, vedi il campo Esc qui sotto.
/// </summary>
internal sealed class StatusBar
{
    private const string LibraryUrl = "https://github.com/mauriziolobello/lare-terminal-2";

    // Carattere ESC (0x1B), costruito con un cast esplicito (char)0x1B invece che con
    // un escape di stringa: evitiamo così qualunque ambiguità fra "\x" (greedy, vedi
    // il commento della classe sopra) e l'escape Unicode a 4 cifre. Definito una sola
    // volta qui e riusato per concatenazione in tutto il resto del file: se in futuro
    // servisse un'altra sequenza VT, si scrive "Esc + "[...codice...]"" invece di
    // reintrodurre un escape di stringa fatto a mano.
    // `internal` (non più `private`) da quando Repl.cs, per l'OSC 9001 dello
    // spike 2 (segnalazione di una riga "/" intercettata verso la finestra
    // Tauri), riusa la stessa identica costante invece di reintrodurre
    // l'escape di stringa manuale che ha causato il bug descritto sopra.
    internal static readonly string Esc = ((char)0x1B).ToString();

    private readonly object _consoleLock;
    private readonly bool _enabled;
    private readonly string[] _spinnerFrames = { "|", "/", "-", "\\" };

    private Thread? _thread;
    private volatile bool _running;
    private volatile string _lastCommand = "(nessuno)";
    private int _spinnerIndex;
    private int _lastWidth;
    private int _lastHeight;

    /// <summary>
    /// Hook statico verso l'UNICA StatusBar attiva nel processo (ce n'è al più una:
    /// Repl.RunInteractive ne crea una sola per l'intera sessione). Serve a codice
    /// che non ha, e non vale la pena far passare esplicitamente, un riferimento
    /// diretto alla StatusBar: LareRawUI.SetBufferContents (Clear-Host deve poter
    /// far ridisegnare le barre dopo aver pulito lo schermo) e
    /// LareHost.NotifyEndApplication (un'app nativa a schermo intero, es.
    /// vim/less/python, potrebbe aver scritto sopra le nostre righe fisse).
    /// Impostato in Start(), azzerato in Stop().
    /// </summary>
    public static StatusBar? Current { get; private set; }

    public StatusBar(object consoleLock)
    {
        _consoleLock = consoleLock;

        // La status bar ha senso solo con una console interattiva reale: se lo
        // stdout è rediretto (pipe, file, o modalità --selftest) non proviamo
        // nemmeno ad abilitare la VT processing o a disegnare le barre.
        _enabled = !Console.IsOutputRedirected && !Console.IsErrorRedirected && ConsoleModes.TryEnableVirtualTerminalProcessing();
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

        Current = this;

        // Il disegno iniziale (regione + due righe + posizionamento del cursore
        // dentro la regione) è esattamente lo stesso lavoro di un ridisegno normale:
        // deleghiamo a Redraw() invece di duplicare la logica qui. Questo garantisce
        // anche il requisito "il cursore deve finire dentro la regione (riga 2..H-1)
        // subito dopo Start()", perché ApplyScrollRegion (chiamata da Redraw) clampa
        // sempre la posizione del cursore dentro i margini.
        Redraw();

        _running = true;
        _thread = new Thread(Loop)
        {
            IsBackground = true, // non deve impedire la chiusura del processo
            Name = "LareStatusBar",
        };
        _thread.Start();
    }

    /// <summary>
    /// Ridisegna entrambe le barre da zero: riapplica la regione di scroll DECSTBM
    /// (il terminale la resetta a "tutta l'altezza" ogni volta che, per qualunque
    /// motivo, il buffer/schermo viene toccato pesantemente: resize, ma anche
    /// Console.Clear() da parte di Clear-Host, o il ritorno dallo schermo alternato
    /// di un'app nativa come vim/less) e ridisegna le due righe fisse.
    /// ApplyScrollRegion si occupa anche di "clampare" il cursore dentro la regione
    /// (righe 2..H-1) se risultasse fuori (tipicamente: riga 1, o l'ultima riga).
    ///
    /// Pubblico e chiamabile da chiunque abbia un riferimento alla StatusBar, oppure
    /// tramite l'hook statico StatusBar.Current per chi non ce l'ha (vedi sopra).
    /// No-op se la StatusBar non è abilitata (console non interattiva).
    /// </summary>
    public void Redraw()
    {
        if (!_enabled)
        {
            return;
        }

        lock (_consoleLock)
        {
            _lastWidth = SafeWidth();
            _lastHeight = SafeHeight();
            ApplyScrollRegion(_lastHeight);
            DrawTopRow();
            DrawBottomRow();
        }
    }

    public void Stop()
    {
        if (!_enabled)
        {
            return;
        }

        _running = false;
        _thread?.Join(TimeSpan.FromMilliseconds(500));
        Current = null;

        lock (_consoleLock)
        {
            // ESC [ r senza parametri = rimuove i margini di scroll (torna a usare
            // tutta l'altezza del buffer). Poi puliamo le due righe che avevamo
            // riservato e ci assicuriamo che il cursore sia visibile.
            var sb = new StringBuilder();
            sb.Append(Esc).Append("[r");
            sb.Append(Esc).Append("7");
            sb.Append(Esc).Append("[1;1H").Append(Esc).Append("[2K");
            sb.Append(Esc).Append("[").Append(_lastHeight).Append(";1H").Append(Esc).Append("[2K");
            sb.Append(Esc).Append("8");
            sb.Append(Esc).Append("[?25h"); // mostra il cursore, per sicurezza
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
                int oldHeight = _lastHeight;
                lock (_consoleLock)
                {
                    // Il terminale non sposta né ridisegna da solo la vecchia barra in
                    // basso quando l'altezza cambia: se non la puliamo esplicitamente
                    // resta visibile come riga residua in inverse video (bug osservato:
                    // "barra duplicata dopo il resize"). La puliamo PRIMA di riapplicare
                    // i margini. La pulizia è avvolta in save/restore cursore (ESC 7 /
                    // ESC 8) perché ApplyScrollRegion, subito dopo, legge la posizione
                    // CORRENTE del cursore per decidere se clampare: se non salvassimo e
                    // ripristinassimo, il cursore risulterebbe spostato dalla pulizia
                    // stessa (sull'ultima riga scritta) e ApplyScrollRegion clamperebbe
                    // sulla base di una posizione "finta", non quella reale dell'utente.
                    var cleanup = new StringBuilder();
                    cleanup.Append(Esc).Append("7");
                    if (oldHeight != height)
                    {
                        cleanup.Append(Esc).Append("[").Append(oldHeight).Append(";1H").Append(Esc).Append("[2K");
                    }
                    cleanup.Append(Esc).Append("[1;1H").Append(Esc).Append("[2K");
                    cleanup.Append(Esc).Append("8");
                    Console.Out.Write(cleanup.ToString());

                    ApplyScrollRegion(height);
                    DrawTopRow();
                    DrawBottomRow();
                }

                _lastWidth = width;
                _lastHeight = height;
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

            Console.Out.Write(Esc + "7" + Esc + "[2;" + bottom + "r" + Esc + "8");

            if (currentRow < 2)
            {
                Console.Out.Write(Esc + "[2;1H");
            }
            else if (currentRow > bottom)
            {
                Console.Out.Write(Esc + "[" + bottom + ";1H");
            }
            // altrimenti: il cursore era già dentro la regione, non lo tocchiamo.
        }
        catch
        {
            // Console non disponibile: impostiamo comunque i margini (utile in
            // --selftest, dove comunque la StatusBar è disabilitata a monte, ma
            // per sicurezza restiamo difensivi) senza tentare di leggere/spostare
            // il cursore.
            Console.Out.Write(Esc + "[2;" + bottom + "r");
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
        sb.Append(Esc).Append("7");           // salva posizione cursore (DECSC, ESC 7)
        sb.Append(Esc).Append("[1;1H");        // vai alla riga 1, colonna 1
        sb.Append(Esc).Append("[2K");          // pulisci l'intera riga
        sb.Append(Esc).Append("[7m").Append(content).Append(Esc).Append("[0m"); // inverse video
        sb.Append(Esc).Append("8");           // ripristina posizione cursore (DECRC, ESC 8)

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
        string libraryLink = Esc + "]8;;" + LibraryUrl + Esc + "\\" + libraryVisible + Esc + "]8;;" + Esc + "\\";
        const string restText = "  /aichat  /config ";

        int visibleLength = helpText.Length + libraryVisible.Length + restText.Length;
        int pad = Math.Max(0, (width - 1) - visibleLength);
        string body = helpText + libraryLink + restText + new string(' ', pad);

        var sb = new StringBuilder();
        sb.Append(Esc).Append("7");
        sb.Append(Esc).Append("[").Append(row).Append(";1H");
        sb.Append(Esc).Append("[2K");
        sb.Append(Esc).Append("[7m").Append(body).Append(Esc).Append("[0m");
        sb.Append(Esc).Append("8");

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

    // --- Abilitazione della VT processing su Windows -------------------------
    // .NET su Windows non abilita di default ENABLE_VIRTUAL_TERMINAL_PROCESSING
    // sull'handle di output standard quando si scrive tramite System.Console in
    // un'app console "vecchio stile": bisogna chiedere esplicitamente a Win32 di
    // interpretare le sequenze ANSI/VT100 che scriviamo, altrimenti verrebbero
    // mostrate come testo letterale (pieno di ESC[...) invece che come comandi.
    // La logica stessa (P/Invoke GetStdHandle/GetConsoleMode/SetConsoleMode) è
    // stata spostata in ConsoleModes.TryEnableVirtualTerminalProcessing() (vedi
    // ConsoleModes.cs): da quando esiste `--no-bars` (spike 2, Tauri) serve poter
    // abilitare la VT processing anche quando questa StatusBar non viene affatto
    // creata, quindi Repl.RunInteractive la richiama direttamente.
}
