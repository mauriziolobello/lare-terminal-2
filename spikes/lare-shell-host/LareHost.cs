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

    public LareHost()
    {
        _ui = new LareHostUI(this);
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
    /// lo script lancia notepad.exe). Un host "serio" li usa per, ad esempio,
    /// ripristinare la modalità raw della console prima di cedere il controllo al
    /// processo figlio. Nel nostro spike non serve: no-op.
    /// </summary>
    public override void NotifyBeginApplication()
    {
        // no-op: nessuna gestione speciale per i processi esterni in questo spike.
    }

    public override void NotifyEndApplication()
    {
        // no-op.
    }
}
