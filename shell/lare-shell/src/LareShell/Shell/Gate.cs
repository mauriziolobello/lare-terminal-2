namespace LareShell.Shell;

/// <summary>Risposta dell'utente al gate ADR-007 (spec §4.3). <c>Cancel</c> = Ctrl+C (annulla tutto
/// il turno, non solo questa richiesta); <c>Abandoned</c> = il turno è finito mentre aspettavamo.</summary>
internal enum GateAnswer
{
    Accept,
    Reject,
    Cancel,
    Abandoned,
}

/// <summary>Confine d'astrazione del prompt [Y/n]: SlashTurn ne dipende, i test passano un finto.</summary>
internal interface IGate
{
    /// <param name="shouldAbandon">Interrogato a ogni giro di attesa: true = smetti di aspettare.</param>
    GateAnswer Ask(string commands, Func<bool> shouldAbandon);
}

/// <summary>
/// Prompt [Y/n] nel terminale con lettura tasto diretta (spec §4.3: "non PSReadLine"). Non blocca
/// in Console.ReadKey: interroga <c>KeyAvailable</c> ogni 50 ms, così può accorgersi (via
/// <paramref name="shouldAbandon"/>) che il turno è già finito. Durante il prompt Ctrl+C arriva
/// come TASTO (TreatControlCAsInput) e vale come annullamento del turno.
/// Invio o Y/y/S/s = accetta (default [Y/n]); N/n = rifiuta.
/// </summary>
internal sealed class ConsoleGate : IGate
{
    public GateAnswer Ask(string commands, Func<bool> shouldAbandon)
    {
        Console.WriteLine();
        Console.ForegroundColor = ConsoleColor.Yellow;
        Console.WriteLine("L'AI propone di eseguire:");
        Console.ResetColor();
        foreach (string line in commands.Replace("\r\n", "\n").Split('\n'))
        {
            Console.WriteLine("  " + line);
        }

        Console.Write("Eseguire? [Y/n] ");

        if (Console.IsInputRedirected)
        {
            // Nessuna tastiera (pipe/test manuale): una riga di testo.
            string? line = Console.ReadLine();
            return line is null or "" || line.StartsWith('y') || line.StartsWith('Y') || line.StartsWith('s') || line.StartsWith('S')
                ? GateAnswer.Accept
                : GateAnswer.Reject;
        }

        bool previous = false;
        try { previous = Console.TreatControlCAsInput; Console.TreatControlCAsInput = true; } catch { /* nessuna console */ }
        try
        {
            while (!shouldAbandon())
            {
                if (!Console.KeyAvailable)
                {
                    Thread.Sleep(50);
                    continue;
                }

                ConsoleKeyInfo key = Console.ReadKey(intercept: true);
                if (key.Key == ConsoleKey.C && key.Modifiers.HasFlag(ConsoleModifiers.Control))
                {
                    Console.WriteLine("^C");
                    return GateAnswer.Cancel;
                }

                if (key.Key == ConsoleKey.Enter || key.KeyChar is 'y' or 'Y' or 's' or 'S')
                {
                    Console.WriteLine("y");
                    return GateAnswer.Accept;
                }

                if (key.KeyChar is 'n' or 'N')
                {
                    Console.WriteLine("n");
                    return GateAnswer.Reject;
                }
            }

            Console.WriteLine();
            Console.WriteLine("(richiesta scaduta: il turno è terminato)");
            return GateAnswer.Abandoned;
        }
        finally
        {
            try { Console.TreatControlCAsInput = previous; } catch { /* nessuna console */ }
        }
    }
}
