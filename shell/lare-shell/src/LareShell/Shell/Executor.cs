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
    /// <summary>Exit code convenzionale per "interrotto da Ctrl+C" (128 + SIGINT), usato solo nei log.</summary>
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

        // $? va letto NELLO stesso script del comando, come ultima istruzione: vale il risultato
        // dell'istruzione precedente, cioè del comando dell'utente (anche per un nativo con exit ≠ 0).
        // Letto da fuori, in una pipeline separata, rifletterebbe la pipeline ESTERNA
        // (… | ForEach-Object | Out-Default), che riesce sempre. L'assegnazione non emette output.
        (bool stopped, bool caughtError) = Invoke(command + "\n$global:" + OkVariable + " = $?", capture);

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
    /// $LASTEXITCODE se ≠ 0, altrimenti 1. Variabile assente = il comando non è arrivato in fondo
    /// (errore terminante dentro lo script) → non ok. Pulisce il segnaposto.</summary>
    private static int ReadExitCode(SessionStateProxy state)
    {
        bool ok = state.GetVariable(OkVariable) is bool b && b;
        int code = state.GetVariable("LASTEXITCODE") is int c ? c : 0;
        state.PSVariable.Remove(OkVariable);
        return ok ? 0 : code != 0 ? code : 1;
    }
}
