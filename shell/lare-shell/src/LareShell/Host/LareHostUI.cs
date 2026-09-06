using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Management.Automation.Host;
using System.Security;

namespace LareShell.Host;

/// <summary>
/// LareHostUI implementa PSHostUserInterface: è il punto in cui il motore PowerShell
/// (cmdlet come Write-Host, Write-Error, Write-Warning, Read-Host, ma anche il
/// formatter di Out-Default per l'output "normale") scrive e legge testo.
///
/// Da qui il motore non sa nulla di System.Console: siamo noi a decidere che,
/// concretamente, scrivere significa scrivere sullo stdout della console con
/// certi colori. Questo è il livello a cui, volendo, si potrebbe reindirizzare
/// tutto l'output verso un'altra destinazione (un file, una finestra grafica, ecc.).
/// </summary>
internal sealed class LareHostUI : PSHostUserInterface
{
    private readonly LareHost _host;
    private readonly LareRawUI _rawUi = new();

    /// <summary>Registratore dell'output per capture:true (spec §4.5 "cattura via
    /// PSHostUserInterface.Write*"): ogni Write* scrive sulla console E, se il registratore è
    /// attivo, anche lì. Così cmdlet e (con il cmdlet di passaggio, Task 4) programmi nativi
    /// tornano all'AI esattamente come li ha visti l'utente.</summary>
    public OutputRecorder Recorder { get; } = new();

    public LareHostUI(LareHost host)
    {
        _host = host;
    }

    public override PSHostRawUserInterface RawUI => _rawUi;

    /// <summary>
    /// Dichiara al motore che questo host sa interpretare le sequenze di escape VT
    /// (colori $PSStyle, ANSI, ecc.) invece di richiedere che siano "spogliate"
    /// prima di arrivare a noi. Dato che abilitiamo esplicitamente la VT processing
    /// sulla console (vedi ConsoleModes.TryEnableVirtualTerminalProcessing), ha senso
    /// dichiararlo.
    /// </summary>
    public override bool SupportsVirtualTerminal => true;

    // --- Scrittura -----------------------------------------------------------

    public override void Write(string value)
    {
        Recorder.Append(value);
        SafeConsole(() => Console.Write(value));
    }

    public override void Write(ConsoleColor foregroundColor, ConsoleColor backgroundColor, string value)
    {
        Recorder.Append(value);
        WriteColored(foregroundColor, backgroundColor, value);
    }

    public override void WriteLine()
    {
        Recorder.Append(Environment.NewLine);
        SafeConsole(() => Console.WriteLine());
    }

    public override void WriteLine(string value)
    {
        Recorder.Append(value + Environment.NewLine);
        SafeConsole(() => Console.WriteLine(value));
    }

    public override void WriteErrorLine(string value) =>
        Write(ConsoleColor.Red, ConsoleColor.Black, value + Environment.NewLine);

    public override void WriteDebugLine(string message) =>
        Write(ConsoleColor.DarkYellow, ConsoleColor.Black, "DEBUG: " + message + Environment.NewLine);

    public override void WriteVerboseLine(string message) =>
        Write(ConsoleColor.DarkCyan, ConsoleColor.Black, "VERBOSE: " + message + Environment.NewLine);

    public override void WriteWarningLine(string message) =>
        Write(ConsoleColor.Yellow, ConsoleColor.Black, "WARNING: " + message + Environment.NewLine);

    /// <summary>
    /// Write-Progress usa questo metodo per aggiornare una barra di progresso.
    /// Qui ci limitiamo a stampare una riga testuale minimale (senza cercare di
    /// disegnare una vera progress bar): è sufficiente per questa versione.
    /// </summary>
    public override void WriteProgress(long sourceId, ProgressRecord record)
    {
        // Come nello spike, ma tramite WriteColored (NON Write): le barre di progresso non vanno
        // nell'output catturato — sarebbero solo rumore per l'AI.
        SafeConsole(() =>
        {
            if (record.RecordType == ProgressRecordType.Completed)
            {
                return;
            }

            string status = record.PercentComplete >= 0
                ? record.Activity + ": " + record.StatusDescription + " (" + record.PercentComplete + "%)"
                : record.Activity + ": " + record.StatusDescription;

            WriteColored(ConsoleColor.DarkGray, ConsoleColor.Black, "[progress] " + status + Environment.NewLine);
        });
    }

    /// <summary>Scrittura colorata sulla console, senza registrazione (ex corpo di Write(colore…)).</summary>
    private static void WriteColored(ConsoleColor foregroundColor, ConsoleColor backgroundColor, string value)
    {
        SafeConsole(() =>
        {
            ConsoleColor prevFg = Console.ForegroundColor;
            ConsoleColor prevBg = Console.BackgroundColor;
            try
            {
                Console.ForegroundColor = foregroundColor;
                Console.BackgroundColor = backgroundColor;
                Console.Write(value);
            }
            finally
            {
                Console.ForegroundColor = prevFg;
                Console.BackgroundColor = prevBg;
            }
        });
    }

    // --- Lettura ---------------------------------------------------------------

    /// <summary>
    /// Usato da Read-Host (senza -AsSecureString). Nota bene: questo NON è il
    /// percorso usato dal REPL per leggere i comandi principali (quello passa da
    /// PSConsoleHostReadLine via PSReadLine, vedi Repl.cs) — questo serve solo
    /// quando uno script chiama esplicitamente Read-Host.
    /// </summary>
    public override string ReadLine() => Console.ReadLine() ?? string.Empty;

    /// <summary>
    /// Usato da Read-Host -AsSecureString. Leggiamo un tasto alla volta, mascherando
    /// l'input con asterischi e senza mai far transitare il testo in chiaro per una
    /// variabile string "normale" (che rimarrebbe in memoria non protetta) più del
    /// minimo indispensabile.
    /// </summary>
    public override SecureString ReadLineAsSecureString()
    {
        var secure = new SecureString();
        while (true)
        {
            ConsoleKeyInfo key;
            try { key = Console.ReadKey(intercept: true); }
            catch { break; }

            if (key.Key == ConsoleKey.Enter)
            {
                Console.Write(Environment.NewLine);
                break;
            }

            if (key.Key == ConsoleKey.Backspace)
            {
                if (secure.Length > 0)
                {
                    secure.RemoveAt(secure.Length - 1);
                    Console.Write("\b \b");
                }
                continue;
            }

            if (!char.IsControl(key.KeyChar))
            {
                secure.AppendChar(key.KeyChar);
                Console.Write("*");
            }
        }

        secure.MakeReadOnly();
        return secure;
    }

    /// <summary>
    /// Prompt() è usato ad es. da cmdlet generati automaticamente per parametri
    /// obbligatori mancanti, o esplicitamente dagli script. Per ogni "campo"
    /// richiesto (FieldDescription) chiediamo un valore via Console e lo
    /// restituiamo come dizionario nome -> PSObject, come richiesto dal contratto.
    /// </summary>
    public override Dictionary<string, PSObject> Prompt(string caption, string message, Collection<FieldDescription> descriptions)
    {
        SafeConsole(() =>
        {
            if (!string.IsNullOrEmpty(caption)) Console.WriteLine(caption);
            if (!string.IsNullOrEmpty(message)) Console.WriteLine(message);
        });

        var results = new Dictionary<string, PSObject>();
        foreach (FieldDescription field in descriptions)
        {
            Console.Write(field.Name + ": ");
            string? input = Console.ReadLine();
            results[field.Name] = PSObject.AsPSObject(input ?? string.Empty);
        }

        return results;
    }

    /// <summary>
    /// PromptForChoice() è usato ad es. da Confirm-Preference o esplicitamente
    /// dagli script (ChoiceDescription[]). Stampiamo le scelte con la lettera
    /// "calda" (accelerator key, marcata con una "e commerciale" nella label
    /// originale) e leggiamo l'indice scelto dall'utente.
    /// </summary>
    public override int PromptForChoice(string caption, string message, Collection<ChoiceDescription> choices, int defaultChoice)
    {
        SafeConsole(() =>
        {
            if (!string.IsNullOrEmpty(caption)) Console.WriteLine(caption);
            if (!string.IsNullOrEmpty(message)) Console.WriteLine(message);
            for (int i = 0; i < choices.Count; i++)
            {
                string marker = i == defaultChoice ? "*" : " ";
                Console.WriteLine(" " + marker + "[" + i + "] " + choices[i].Label.Replace("&", string.Empty) + "  - " + choices[i].HelpMessage);
            }
        });

        while (true)
        {
            Console.Write("Scelta (0-" + (choices.Count - 1) + ", default " + defaultChoice + "): ");
            string? line = Console.ReadLine();
            if (string.IsNullOrWhiteSpace(line)) return defaultChoice;
            if (int.TryParse(line, out int idx) && idx >= 0 && idx < choices.Count) return idx;
            Console.WriteLine("Scelta non valida, riprova.");
        }
    }

    // --- Credenziali: non implementate --------------------------------------
    // Richiederebbero UI dedicata (mascheramento username/password, eventuale
    // integrazione con Credential Manager di Windows). Fuori MVP: non servono per
    // il funzionamento di base dell'host, quindi restano NotImplemented.

    public override PSCredential PromptForCredential(string caption, string message, string userName, string targetName) =>
        throw new NotImplementedException("PromptForCredential non è implementato (fuori MVP).");

    public override PSCredential PromptForCredential(
        string caption, string message, string userName, string targetName,
        PSCredentialTypes allowedCredentialTypes, PSCredentialUIOptions options) =>
        throw new NotImplementedException("PromptForCredential (overload esteso) non è implementato (fuori MVP).");

    // --- Helper -----------------------------------------------------------

    private static void SafeConsole(Action action)
    {
        try { action(); }
        catch
        {
            // Console non disponibile (stdout rediretto, nessun terminale, ecc.):
            // ignoriamo per non far crashare l'host durante --selftest o in pipe.
        }
    }
}
