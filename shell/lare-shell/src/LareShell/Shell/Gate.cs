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
            // Nessuna tastiera (pipe/test manuale): una riga di testo. Il gate fallisce CHIUSO:
            // EOF (ReadLine → null) vuol dire che non c'è nessun umano a rispondere, e un gate di
            // conferma (ADR-007) in dubbio deve rifiutare, mai accettare. Una riga VUOTA invece è
            // l'Invio di un utente vero e vale come il default del prompt [Y/n], cioè accetta.
            // Limite accettato: in questo ramo si resta fermi dentro ReadLine senza consultare
            // shouldAbandon (nessun polling possibile su una pipe), quindi il prompt non si chiude
            // da solo quando il turno finisce. Stdin rediretto è un percorso di sviluppo/test, non
            // del prodotto — e comunque il timeout di 180 s lato orchestratore (spec §4.3) chiude
            // il turno per conto suo.
            string? line = Console.ReadLine();
            if (line is null)
            {
                return GateAnswer.Reject;
            }

            return line is "" || line.StartsWith('y') || line.StartsWith('Y') || line.StartsWith('s') || line.StartsWith('S')
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
