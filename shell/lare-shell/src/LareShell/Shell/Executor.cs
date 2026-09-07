using System.Management.Automation;
using System.Management.Automation.Runspaces;

namespace LareShell.Shell;

/// <summary>Esito di un <c>ExecInShell</c> (spec §4.5): <c>Output</c> vuoto con capture:false;
/// <c>Stopped</c> = fermato da Ctrl+C (niente ExecResult: il turno va cancellato, §4.4).</summary>
internal sealed record ExecOutcome(int ExitCode, string Output, string Cwd, bool Stopped);

/// <summary>Confine d'astrazione: SlashTurn (Task 5) dipende da questo, i test gli passano un falso.</summary>
internal interface IExecutor
{
    ExecOutcome Run(string command, bool capture);

    void StopCurrent();
}

/// <summary>
/// Esegue comandi nel runspace dell'utente. Due modalità (spec §4.5, lezione dello spike):
///  - capture:true  → <c>script | ForEach-Object { $_ } | Out-Default</c>: il cmdlet in mezzo fa
///    sì che un programma nativo NON sia l'ultimo della pipeline, quindi il suo stdout passa
///    dalla pipe → Out-Default → LareHostUI.Write → Recorder; l'output torna all'AI.
///  - capture:false → <c>script | Out-Default</c> puro: il nativo eredita la console vera
///    (editor, REPL, wizard funzionano); output vuoto, restano exit_code e cwd.
/// Errori: Error→Output (MergeMyResults, come ConsoleHost) così gli ErrorRecord sono formattati
/// in rosso e registrati. Va chiamato SOLO dal thread del REPL (una runspace = una pipeline alla volta).
/// </summary>
internal sealed class Executor : IExecutor
{
    /// <summary>Exit code convenzionale per "interrotto da Ctrl+C" (128 + SIGINT): valore
    /// convenzionale restituito in ExecOutcome quando la pipeline è stata fermata (il chiamante
    /// manda CancelCommand, mai un ExecResult).</summary>
    private const int StoppedExitCode = 130;

    private readonly RunspaceSession _session;
    private volatile PowerShell? _current;

    public Executor(RunspaceSession session) => _session = session;

    /// <summary>Segnaposto globale in cui lo script cattura il proprio $? (vedi Run).</summary>
    private const string OkVariable = "__lare_ok";

    public ExecOutcome Run(string command, bool capture)
    {
        if (capture)
        {
            _session.Host.HostUI.Recorder.Begin();
        }

        // Prima del comando: $LASTEXITCODE azzerato, così un valore ≠ 0 letto DOPO appartiene a
        // QUESTO comando e non a un nativo fallito in un turno precedente; il segnaposto di $?
        // rimosso, così se il comando non arriva in fondo (errore terminante) la variabile manca.
        SessionStateProxy state = _session.Runspace.SessionStateProxy;
        state.SetVariable("LASTEXITCODE", null);
        state.PSVariable.Remove(OkVariable);

        // Il comando dell'utente gira dentro "try { ... } finally { $global:__lare_ok = $? }",
        // non appeso direttamente allo script. Tre fatti, verificati empiricamente (Task 4, fix
        // round 1 — un semplice "sembra giusto" non basta con l'engine PowerShell):
        //  1. Perché non un append diretto ("<command>\n$global:__lare_ok = $?", il design
        //     originale): un "return" a livello superiore del comando (fuori da una funzione, es.
        //     "if (...) { return }") termina TUTTO lo script prima di raggiungere quella riga — un
        //     comando RIUSCITO che finisce con return tornava comunque exit_code 1 (difetto trovato
        //     in review).
        //  2. Perché non ". { ... }" o "& { ... }" (un blocco dot-sourced o invocato attorno al
        //     comando, che risolverebbe il return): invocare O dot-sourcere un blocco "{ ... }" è
        //     un CONFINE DI CHIAMATA per il motore — al suo ritorno $? diventa SEMPRE true,
        //     qualunque cosa sia successa dentro (un nativo con exit ≠ 0, un errore non
        //     terminante…), a meno che il blocco stesso lanci un'eccezione. Verificato con una
        //     diagnostica dedicata: ". { cmd /c exit 4 }" e "& { cmd /c exit 4 }" davano entrambi
        //     $?=True subito dopo, mentre lo stesso comando senza blocco dava $?=False. Con quel
        //     meccanismo un nativo o un cmdlet falliti dentro il blocco sarebbero tornati come
        //     SUCCESSO — avrebbe rotto esattamente i due test che il brief segnalava come a rischio
        //     (nativo con capture:true, capture:false).
        //  3. Perché "try { ... } finally { ... }" risolve tutto senza introdurre il problema del
        //     punto 2: try/finally è normale flusso di controllo dentro allo STESSO script, NON una
        //     chiamata a un blocco separato — non è un confine di chiamata, quindi non apre un
        //     nuovo scope (variabili/funzioni definite dal comando restano nella sessione dopo,
        //     esattamente come digitandolo al prompt) e $? dentro "finally" resta quello della vera
        //     ultima istruzione eseguita nel "try" (nativo fallito o errore non terminante inclusi).
        //     "return" dentro "try" esegue comunque "finally" prima di uscire (garanzia del
        //     linguaggio): la cattura di $? avviene sempre, anche quando il comando finisce con
        //     return.
        // Limite noto, documentato (non aggirato qui): un "exit" dentro il comando dell'AI non è
        // fermato dal try/finally — esce dal PROCESSO come farebbe un "exit" digitato al prompt
        // dell'utente, arrivando a LareHost.SetShouldExit. Il gate di conferma ha già mostrato il
        // comando prima dell'esecuzione: è un limite dichiarato, non un buco di sicurezza.
        (bool stopped, bool caughtError) = Invoke(
            "try {\n" + command + "\n} finally {\n$global:" + OkVariable + " = $?\n}", capture);

        string output = capture ? _session.Host.HostUI.Recorder.End() : string.Empty;
        int exitCode = stopped ? StoppedExitCode : caughtError ? 1 : ReadExitCode(state);
        return new ExecOutcome(exitCode, output, _session.CurrentDirectory, stopped);
    }

    /// <summary>Comando digitato dall'utente al prompt: pipeline pura, nessuna cattura, nessuna
    /// istruzione aggiunta, $LASTEXITCODE/$? intatti (che resterebbero alterati per la funzione
    /// prompt — ruling 3). Ritorna true se fermato da Ctrl+C.</summary>
    public bool RunInteractive(string line) => Invoke(line, capture: false).Stopped;

    public void StopCurrent() => _current?.Stop();

    private (bool Stopped, bool CaughtError) Invoke(string script, bool capture)
    {
        using var ps = PowerShell.Create();
        ps.Runspace = _session.Runspace;
        _current = ps;
        try
        {
            ps.AddScript(script, useLocalScope: false);
            ps.Commands.Commands[0].MergeMyResults(PipelineResultTypes.Error, PipelineResultTypes.Output);
            if (capture)
            {
                ps.AddCommand("ForEach-Object").AddParameter("Process", ScriptBlock.Create("$_"));
            }

            ps.AddCommand("Out-Default");
            ps.Invoke();

            // con Microsoft.PowerShell.SDK 7.6.5 Stop() fa tornare Invoke() normalmente con stato
            // Stopped, non con PipelineStoppedException — scoperto nel Task 4. Il catch sotto
            // resta comunque (copre un eventuale stop che arrivi come eccezione, es. altre
            // versioni dell'SDK o l'API Pipeline usata altrove): i due percorsi sono mutuamente
            // esclusivi (uno è nel try, l'altro nel catch), quindi la riga "[LARE] comando
            // interrotto" non può mai raddoppiare per la stessa invocazione.
            if (ps.InvocationStateInfo.State == PSInvocationState.Stopped)
            {
                _session.Host.UI.WriteLine("[LARE] comando interrotto (Ctrl+C).");
                return (true, false);
            }

            return (false, false);
        }
        catch (PipelineStoppedException)
        {
            // ps.Stop() da Ctrl+C (è una RuntimeException: va catturata PRIMA).
            _session.Host.UI.WriteLine("[LARE] comando interrotto (Ctrl+C).");
            return (true, false);
        }
        catch (RuntimeException ex)
        {
            // ParseException (sintassi: fallisce prima di partire, non passa dal merge) e gli
            // errori terminanti fuori pipeline: mostrati e registrati come errore.
            _session.Host.UI.WriteErrorLine(ex.Message);
            return (false, true);
        }
        finally
        {
            _current = null;
        }
    }

    /// <summary>Regola (ruling 3): 0 se il $? catturato dallo script è vero; altrimenti
    /// $LASTEXITCODE se ≠ 0, altrimenti 1. Con "try/finally" il segnaposto è quasi sempre presente
    /// (il "finally" lo imposta anche se il "try" fallisce) — il caso "variabile assente" resta
    /// solo per un errore che impedisce l'esecuzione del "finally" stesso (es. ParseException:
    /// quella strada torna 1 PRIMA di chiamare questo metodo, tramite <c>caughtError</c> in
    /// <see cref="Run"/>, non passando di qui). Pulisce il segnaposto.</summary>
    private static int ReadExitCode(SessionStateProxy state)
    {
        bool ok = state.GetVariable(OkVariable) is bool b && b;
        int code = state.GetVariable("LASTEXITCODE") is int c ? c : 0;
        state.PSVariable.Remove(OkVariable);
        return ok ? 0 : code != 0 ? code : 1;
    }
}
