using System.Globalization;
using System.Management.Automation.Host;

namespace LareShellSpike;

/// <summary>
/// LareHost è l'implementazione minima di PSHost: rappresenta "l'applicazione ospite"
/// agli occhi del motore PowerShell. Il motore usa questa classe per sapere chi è
/// l'host (Name/Version), per ottenere l'interfaccia utente (UI) a cui scrivere
/// output/errori, e per essere notificato di eventi come "l'utente ha digitato exit"
/// (SetShouldExit) o "sto per lanciare un programma esterno" (NotifyBeginApplication).
///
/// Questo è esattamente il ruolo che in PowerShell.exe reale svolge la classe
/// ConsoleHost (vedi src/Microsoft.PowerShell.ConsoleHost/host/msh/ConsoleHost.cs
/// nel repo di riferimento): PSHost è il "contratto" minimo che un host deve
/// rispettare per poter ospitare il motore.
/// </summary>
internal sealed class LareHost : PSHost
{
    // Un Guid univoco per questa istanza dell'host: il motore lo usa per distinguere
    // host diversi (es. in scenari di logging/telemetria). Non ha altro significato.
    private readonly Guid _instanceId = Guid.NewGuid();

    // L'interfaccia utente: è qui che deleghiamo tutte le operazioni di I/O
    // (scrittura, lettura, prompt, colori...). PSHost NON sa nulla di "Console":
    // è LareHostUI a tradurre le richieste del motore in chiamate a System.Console.
    private readonly LareHostUI _ui;

    // --- Stato per NotifyBeginApplication/NotifyEndApplication --------------
    // Vedi il commento sui due metodi più sotto per la spiegazione completa. Qui
    // teniamo solo i campi: le console mode "iniziali" (catturate qui nel
    // costruttore, PRIMA che StatusBar abiliti la virtual terminal processing
    // sull'output - in Repl.cs l'host viene costruito prima della StatusBar), le
    // mode "salvate" nel momento in cui un'app nativa parte (per poterle
    // ripristinare quando finisce) e un contatore di nesting (un'app nativa
    // potrebbe lanciarne un'altra).
    private readonly bool _consoleModesAvailable;
    private readonly uint _initialOutputMode;
    private readonly uint _initialInputMode;
    private bool _savedModesValid;
    private uint _savedOutputMode;
    private uint _savedInputMode;
    private int _beginApplicationNotifyCount;
    private readonly object _appNotifyLock = new();

    public LareHost()
    {
        _ui = new LareHostUI(this);

        // Catturiamo qui le console mode "di partenza". Solo Windows: le altre
        // piattaforme non hanno il concetto di "console mode" via kernel32, e
        // OperatingSystem.IsWindows() viene ricontrollato in Notify*Application.
        if (OperatingSystem.IsWindows())
        {
            try
            {
                IntPtr outHandle = ConsoleModes.GetHandle(ConsoleModes.StdOutputHandle);
                IntPtr inHandle = ConsoleModes.GetHandle(ConsoleModes.StdInputHandle);

                // Usiamo "&" (non "&&") apposta: vogliamo che ENTRAMBE le GetMode
                // vengano tentate anche se la prima fallisce, non un cortocircuito.
                _consoleModesAvailable =
                    ConsoleModes.TryGetMode(outHandle, out _initialOutputMode) &
                    ConsoleModes.TryGetMode(inHandle, out _initialInputMode);
            }
            catch
            {
                // Nessuna console reale (--selftest, pipe...): NotifyBeginApplication
                // e NotifyEndApplication diventeranno no-op, vedi il controllo su
                // _consoleModesAvailable in entrambi.
                _consoleModesAvailable = false;
            }
        }
    }

    // --- Identità dell'host -------------------------------------------------

    public override string Name => "LareShellSpike";

    public override Version Version { get; } = new Version(0, 1, 0);

    public override Guid InstanceId => _instanceId;

    // Culture usate dal motore per formattazione numeri/date e per i messaggi
    // localizzati dei cmdlet. Usiamo semplicemente la culture del thread corrente.
    public override CultureInfo CurrentCulture => CultureInfo.CurrentCulture;

    public override CultureInfo CurrentUICulture => CultureInfo.CurrentUICulture;

    public override PSHostUserInterface UI => _ui;

    // --- Ciclo di vita / stato "should exit" --------------------------------

    /// <summary>
    /// Diventa true quando lo script/l'utente ha invocato il comando "exit".
    /// Il loop REPL (Repl.cs) controlla questa proprietà dopo ogni comando eseguito
    /// e, se è true, esce dal ciclo di lettura-esecuzione.
    /// </summary>
    public bool ShouldExit { get; private set; }

    public int ExitCode { get; private set; }

    /// <summary>
    /// Chiamato dal motore PowerShell quando viene eseguito il comando "exit"
    /// (o Environment.Exit implicito da uno script). Non usciamo subito dal
    /// processo qui: ci limitiamo a segnalarlo, così il chiamante (Repl) può
    /// chiudere in modo pulito la runspace e ripristinare la console.
    /// </summary>
    public override void SetShouldExit(int exitCode)
    {
        ShouldExit = true;
        ExitCode = exitCode;
    }

    /// <summary>
    /// I "nested prompt" servono per scenari come il debugger di PowerShell
    /// (quando ti fermi a un breakpoint ottieni un prompt annidato "[DBG]: PS>").
    /// Per questo spike non li implementiamo: non ci serve il debugging interattivo.
    /// </summary>
    public override void EnterNestedPrompt() => throw new NotImplementedException(
        "EnterNestedPrompt non è implementato in questo spike (nessun supporto al debugger).");

    public override void ExitNestedPrompt() => throw new NotImplementedException(
        "ExitNestedPrompt non è implementato in questo spike (nessun supporto al debugger).");

    /// <summary>
    /// Chiamati dal motore prima/dopo l'avvio di un programma esterno (es. quando
    /// lo script lancia python.exe o notepad.exe).
    ///
    /// Bug osservato con questo spike PRIMA di questo fix: lanciare `python`
    /// (il suo REPL interattivo, un'app nativa) produceva un flusso infinito di
    /// errori finché non si premeva Ctrl+C. Replica lo stesso pattern del
    /// `ConsoleHost` reale (vedi src/Microsoft.PowerShell.ConsoleHost/host/msh/
    /// ConsoleHost.cs ~1220-1275 nel repo di riferimento PowerShell:
    /// NotifyBeginApplication/NotifyEndApplication con _initialConsoleMode/
    /// _savedConsoleMode e un contatore di nesting), con una differenza voluta:
    /// ConsoleHost reale tocca solo l'handle di OUTPUT (GetActiveScreenBufferHandle);
    /// qui tocchiamo anche l'handle di INPUT (stdin, handle -10), perché in questo
    /// spike anche la modalità di INPUT viene alterata (Console/PSReadLine la
    /// mettono in modalità "raw" per leggere tasto per tasto) e un'app nativa
    /// interattiva come il REPL di `python` si aspetta, all'avvio, la stessa
    /// modalità di input "di sistema" che aveva pwsh.exe all'avvio.
    ///
    /// Il contatore (_beginApplicationNotifyCount) gestisce il caso in cui un'app
    /// nativa ne lanci un'altra (nesting): ripristiniamo le mode "originali" solo
    /// quando l'ULTIMA app nativa attiva termina (il contatore torna a 0), non alla
    /// prima NotifyEndApplication.
    /// </summary>
    public override void NotifyBeginApplication()
    {
        if (!OperatingSystem.IsWindows() || !_consoleModesAvailable)
        {
            return;
        }

        lock (_appNotifyLock)
        {
            if (++_beginApplicationNotifyCount == 1)
            {
                try
                {
                    IntPtr outHandle = ConsoleModes.GetHandle(ConsoleModes.StdOutputHandle);
                    IntPtr inHandle = ConsoleModes.GetHandle(ConsoleModes.StdInputHandle);

                    // Salviamo la mode CORRENTE (quella che StatusBar/PSReadLine hanno
                    // impostato finora) per poterla ripristinare in NotifyEndApplication.
                    bool outOk = ConsoleModes.TryGetMode(outHandle, out _savedOutputMode);
                    bool inOk = ConsoleModes.TryGetMode(inHandle, out _savedInputMode);
                    _savedModesValid = outOk && inOk;

                    // Riportiamo entrambi gli handle alla mode "di partenza" catturata
                    // nel costruttore, così l'app nativa vede una console come all'avvio.
                    ConsoleModes.TrySetMode(outHandle, _initialOutputMode);
                    ConsoleModes.TrySetMode(inHandle, _initialInputMode);
                }
                catch
                {
                    // Difensivo: se qualcosa va storto qui non vogliamo comunque
                    // impedire l'avvio dell'app nativa.
                }
            }
        }
    }

    /// <summary>Vedi il commento su NotifyBeginApplication qui sopra.</summary>
    public override void NotifyEndApplication()
    {
        if (!OperatingSystem.IsWindows() || !_consoleModesAvailable)
        {
            return;
        }

        lock (_appNotifyLock)
        {
            if (--_beginApplicationNotifyCount == 0)
            {
                try
                {
                    IntPtr outHandle = ConsoleModes.GetHandle(ConsoleModes.StdOutputHandle);
                    IntPtr inHandle = ConsoleModes.GetHandle(ConsoleModes.StdInputHandle);

                    if (_savedModesValid)
                    {
                        ConsoleModes.TrySetMode(inHandle, _savedInputMode);

                        // Ripristiniamo l'output e, per sicurezza, ci assicuriamo che
                        // ENABLE_VIRTUAL_TERMINAL_PROCESSING resti accesa: un'app nativa
                        // "full screen" (es. vim/less, o lo stesso python) potrebbe
                        // averla spenta senza rimetterla come l'aveva trovata.
                        uint restoredOutputMode = _savedOutputMode
                            | ConsoleModes.EnableVirtualTerminalProcessing
                            | ConsoleModes.EnableProcessedOutput;
                        ConsoleModes.TrySetMode(outHandle, restoredOutputMode);
                    }
                    else
                    {
                        // Non siamo riusciti a leggere le mode salvate in
                        // NotifyBeginApplication (_savedModesValid è false): come
                        // fallback ripristiniamo almeno le mode iniziali, meglio che
                        // lasciare la console nello stato lasciato dall'app nativa.
                        ConsoleModes.TrySetMode(inHandle, _initialInputMode);
                        uint fallbackOutputMode = _initialOutputMode
                            | ConsoleModes.EnableVirtualTerminalProcessing
                            | ConsoleModes.EnableProcessedOutput;
                        ConsoleModes.TrySetMode(outHandle, fallbackOutputMode);
                    }
                }
                catch
                {
                    // Difensivo, vedi NotifyBeginApplication.
                }

                // Un'app "full screen" (vim/less/python) potrebbe aver usato lo schermo
                // alternato o comunque aver scritto sopra le nostre due righe fisse:
                // ridisegniamole. StatusBar.Current è l'hook statico impostato da
                // StatusBar.Start() (vedi StatusBar.cs): se non c'è nessuna StatusBar
                // attiva (--selftest, o StatusBar disabilitata) Redraw() è no-op/null-safe.
                StatusBar.Current?.Redraw();
            }
        }
    }
}
