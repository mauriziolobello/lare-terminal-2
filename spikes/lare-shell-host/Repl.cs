using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Management.Automation.Runspaces;

namespace LareShellSpike;

/// <summary>
/// Repl contiene il ciclo read-eval-print-loop vero e proprio: crea la runspace
/// ospitata (con PSReadLine importato), legge le righe di comando esattamente come
/// fa pwsh.exe (invocando la funzione PSConsoleHostReadLine), intercetta le righe
/// che iniziano con "/" senza eseguirle in PowerShell, ed esegue tutto il resto
/// nella runspace mostrando sia l'output "normale" sia un conteggio degli oggetti
/// prodotti.
/// </summary>
internal static class Repl
{
    /// <summary>
    /// Oggetto di lock condiviso con StatusBar: sia il thread della status bar sia
    /// il thread principale (che scrive prompt/output/errori) scrivono sulla stessa
    /// System.Console. Senza un lock condiviso, un refresh della barra a intervalli
    /// di 1s potrebbe intercalarsi a metà di una scrittura del thread principale
    /// (o di PSReadLine) producendo output corrotto sullo schermo.
    /// </summary>
    private static readonly object ConsoleLock = new();

    // Riferimento al comando PowerShell attualmente in esecuzione: serve per poterlo
    // fermare (ps.Stop()) quando l'utente preme Ctrl+C. È volatile perché viene letto
    // dal thread dell'handler di CancelKeyPress, diverso dal thread del loop principale.
    private static volatile PowerShell? _currentPipeline;

    /// <summary>
    /// Riconosce e "spiega" le righe intercettate (quelle che iniziano con "/").
    /// È una funzione pura e statica, condivisa fra il loop interattivo (Repl.RunInteractive)
    /// e la modalità --selftest (Program.cs), così dimostriamo che è la STESSA logica
    /// di intercettazione ad essere testata sia manualmente sia in modo automatico.
    /// </summary>
    /// <param name="rawLine">La riga così come digitata dall'utente (non trimmata).</param>
    /// <param name="message">Il messaggio da stampare se la riga è stata intercettata.</param>
    /// <returns>true se la riga inizia con "/" (dopo trim) e quindi va intercettata.</returns>
    public static bool TryIntercept(string rawLine, out string message)
    {
        string trimmed = rawLine.TrimStart();
        if (trimmed.StartsWith('/'))
        {
            message = "[LARE] intercettato: " + rawLine;
            return true;
        }

        message = string.Empty;
        return false;
    }

    /// <summary>
    /// Avvia il loop interattivo. Ritorna il codice di uscita del processo.
    /// </summary>
    public static int RunInteractive()
    {
        // --- 1. Costruzione dello stato iniziale della sessione -------------
        // InitialSessionState.CreateDefault() carica tutti i moduli/snap-in di
        // default che caricherebbe una sessione PowerShell "normale" (Microsoft.
        // PowerShell.Management, Utility, ecc.). ImportPSModule aggiunge PSReadLine
        // alla lista dei moduli da importare all'apertura della runspace: è lo
        // stesso identico pattern usato da ConsoleHost.cs (vedi
        // DefaultInitialSessionState.ImportPSModule(new[] { "PSReadLine" })).
        InitialSessionState iss = InitialSessionState.CreateDefault();
        iss.ImportPSModule(new[] { "PSReadLine" });

        // CreateDefault() non forza una execution policy: la runspace eredita quella
        // di sistema per lo scope corrente, che su questa macchina risulta
        // "Restricted" per un processo host che non è pwsh.exe (blocca anche il
        // caricamento di PSReadLine.psm1). La impostiamo esplicitamente qui: è la
        // scelta comune per un'applicazione che ospita il motore PowerShell.
        iss.ExecutionPolicy = Microsoft.PowerShell.ExecutionPolicy.RemoteSigned;

        var host = new LareHost();
        using Runspace runspace = RunspaceFactory.CreateRunspace(host, iss);
        runspace.Open();

        // Runspace.DefaultRunspace deve essere impostato sul thread corrente perché
        // molte API "ambient" di PowerShell (es. alcuni moduli, [PowerShell]::Create()
        // senza specificare una Runspace) assumono che esista una runspace di default
        // per il thread in corso. ConsoleHost fa lo stesso nel suo thread di UI.
        Runspace.DefaultRunspace = runspace;

        var statusBar = new StatusBar(ConsoleLock);

        // NOTA IMPORTANTE (osservata empiricamente durante lo sviluppo di questo
        // spike): se lo stdin è rediretto (es. `echo "/exit" | lare-shell-spike.exe`),
        // invocare la funzione PSConsoleHostReadLine NON restituisce mai EOF e NON
        // consuma le righe della pipe: torna ripetutamente una riga vuota, causando
        // un loop infinito di prompt (ce ne siamo accorti perché il processo è
        // dovuto essere ucciso da un timeout esterno). Il ConsoleHost reale evita
        // esattamente questo problema: LoadPSReadline() in ConsoleHost.cs controlla
        // "stdin is redirected by a parent process" fra le condizioni per NON
        // caricare/usare PSReadLine. Replichiamo qui la stessa guardia.
        bool inputRedirected = Console.IsInputRedirected;
        bool psReadLineAvailable = !inputRedirected && FunctionExists(runspace, "PSConsoleHostReadLine");
        if (inputRedirected)
        {
            Console.WriteLine("[LARE] stdin rediretto: PSReadLine non verrà usato per leggere le righe (fallback a Console.ReadLine), per evitare il loop infinito osservato altrimenti.");
        }

        lock (ConsoleLock)
        {
            Console.WriteLine("Lare Terminal 2.0 - spike host PowerShell");
            Console.WriteLine("PSReadLine disponibile: " + (psReadLineAvailable ? "si" : "NO (fallback a Console.ReadLine)"));
            Console.WriteLine("Righe che iniziano con '/' vengono intercettate, non eseguite. '/exit' esce.");
            Console.WriteLine();
        }

        // Ctrl+C: invece di terminare bruscamente il processo, fermiamo solo la
        // pipeline PowerShell eventualmente in esecuzione (comportamento identico a
        // quello di pwsh.exe: Ctrl+C interrompe il comando corrente, non la shell).
        ConsoleCancelEventHandler cancelHandler = (_, e) =>
        {
            e.Cancel = true;
            _currentPipeline?.Stop();
        };
        Console.CancelKeyPress += cancelHandler;

        try
        {
            DotSourceProfiles(runspace, host);
            statusBar.Start();

            while (!host.ShouldExit)
            {
                string prompt = EvaluatePrompt(runspace);
                lock (ConsoleLock)
                {
                    Console.Write(prompt);
                }

                string? line = ReadLine(runspace, psReadLineAvailable);
                if (line is null)
                {
                    // EOF sullo stdin (es. input rediretto/pipe esaurita): usciamo dal
                    // loop in modo pulito, altrimenti un `echo "/exit" | dotnet run`
                    // girerebbe all'infinito rileggendo righe vuote.
                    lock (ConsoleLock) { Console.WriteLine(); Console.WriteLine("[LARE] EOF su stdin, uscita."); }
                    break;
                }

                if (TryIntercept(line, out string interceptMessage))
                {
                    lock (ConsoleLock)
                    {
                        Console.ForegroundColor = ConsoleColor.Cyan;
                        Console.WriteLine(interceptMessage);
                        Console.ResetColor();
                    }
                    statusBar.SetLastCommand(line.Trim());

                    if (string.Equals(line.Trim(), "/exit", StringComparison.OrdinalIgnoreCase))
                    {
                        break;
                    }

                    continue;
                }

                if (string.IsNullOrWhiteSpace(line))
                {
                    continue;
                }

                ExecuteAndCount(runspace, host, line);
            }
        }
        finally
        {
            Console.CancelKeyPress -= cancelHandler;
            statusBar.Stop();
            Runspace.DefaultRunspace = null;
        }

        return host.ExitCode;
    }

    /// <summary>
    /// Legge una riga di comando usando PSReadLine (invocando la funzione
    /// PSConsoleHostReadLine nella runspace, esattamente come fa
    /// ConsoleHostUserInterface.TryInvokeUserDefinedReadLine nel host reale).
    /// Se PSReadLine non è disponibile, o l'invocazione fallisce per qualsiasi
    /// motivo, ripieghiamo su Console.ReadLine().
    /// </summary>
    private static string? ReadLine(Runspace runspace, bool psReadLineAvailable)
    {
        if (psReadLineAvailable)
        {
            try
            {
                using var ps = PowerShell.Create();
                ps.Runspace = runspace;
                Collection<PSObject> result = ps.AddCommand("PSConsoleHostReadLine").Invoke();
                if (result.Count == 1)
                {
                    return result[0].BaseObject as string ?? string.Empty;
                }

                // 0 risultati con PSReadLine di solito significa Ctrl+C durante
                // l'editing della riga: trattiamolo come riga vuota, non come EOF.
                return string.Empty;
            }
            catch (Exception ex)
            {
                lock (ConsoleLock)
                {
                    Console.ForegroundColor = ConsoleColor.DarkYellow;
                    Console.WriteLine();
                    Console.WriteLine("[LARE] PSConsoleHostReadLine ha fallito (" + ex.GetType().Name + "), fallback a Console.ReadLine per questa riga.");
                    Console.ResetColor();
                }
            }
        }

        return Console.ReadLine();
    }

    /// <summary>
    /// Verifica, con un semplice Get-Command, se una funzione/cmdlet con il nome
    /// dato esiste nella runspace. Usato sia per capire se PSReadLine si è caricato
    /// correttamente sia (in modalità --selftest) per il check [OK]/[FAIL].
    /// </summary>
    public static bool FunctionExists(Runspace runspace, string name)
    {
        try
        {
            using var ps = PowerShell.Create();
            ps.Runspace = runspace;
            ps.AddCommand("Get-Command")
              .AddParameter("Name", name)
              .AddParameter("ErrorAction", "SilentlyContinue");
            Collection<PSObject> result = ps.Invoke();
            return result.Count > 0;
        }
        catch
        {
            return false;
        }
    }

    /// <summary>
    /// Valuta la funzione "prompt" nella runspace (definita di default da PowerShell,
    /// eventualmente ridefinita dal profilo utente) esattamente come fa ConsoleHost
    /// in EvaluatePrompt(). Se fallisce o non produce output, usiamo un prompt di
    /// fallback "PS &lt;cwd&gt;&gt; ".
    /// </summary>
    private static string EvaluatePrompt(Runspace runspace)
    {
        try
        {
            using var ps = PowerShell.Create();
            ps.Runspace = runspace;
            Collection<PSObject> result = ps.AddCommand("prompt").Invoke();
            if (result.Count > 0)
            {
                string? text = result[0].BaseObject as string;
                if (!string.IsNullOrEmpty(text))
                {
                    return text;
                }
            }
        }
        catch
        {
            // Ignoriamo: usiamo il prompt di default sotto.
        }

        return "PS " + Directory.GetCurrentDirectory() + "> ";
    }

    /// <summary>
    /// Esegue la riga NON intercettata nella runspace ospitata e mostra sia
    /// l'output "normale" (via Out-Default, quindi con la stessa formattazione
    /// a colori che vedresti in pwsh.exe) sia un conteggio degli oggetti prodotti.
    ///
    /// Tecnica usata (spiegata perché è la parte concettualmente più delicata):
    /// pipeline = AddScript(riga) | Tee-Object -Variable __LareLastOutput | Out-Default
    /// Tee-Object duplica ogni oggetto che passa nella pipeline: una copia va,
    /// come al solito, a Out-Default (quindi l'utente vede l'output esattamente
    /// come lo vedrebbe in pwsh normale, in streaming), l'altra copia viene
    /// accumulata in una variabile della sessione. Dopo l'esecuzione, una seconda
    /// (piccolissima) invocazione PowerShell legge quella variabile per contare
    /// gli oggetti e poi la rimuove. Questo NON è un "secondo passaggio" sul
    /// comando dell'utente (che gira una volta sola, in streaming): è solo una
    /// letturaisurina della variabile che Tee-Object ha già popolato.
    ///
    /// Il conteggio esclude i record di errore (ErrorRecord), che finiscono nella
    /// stessa pipeline a causa del merge error->output (vedi sotto): altrimenti un
    /// comando che produce solo errori risulterebbe fuorviante mente "produce
    /// oggetti".
    /// </summary>
    private static void ExecuteAndCount(Runspace runspace, LareHost host, string line)
    {
        using var ps = PowerShell.Create();
        ps.Runspace = runspace;
        _currentPipeline = ps;

        try
        {
            ps.AddScript(line, useLocalScope: false);

            // Uniamo il flusso di errore della PRIMA istruzione della pipeline nel
            // flusso di output: è lo stesso identico pattern usato da ConsoleHost
            // (Executor.cs: "tempPipeline.Commands[0].MergeMyResults(Error, Output)").
            // In questo modo gli ErrorRecord vengono formattati e stampati da
            // Out-Default con lo stile "errore" (testo rosso), invece di finire
            // silenziosamente in ps.Streams.Error senza mai essere mostrati.
            ps.Commands.Commands[0].MergeMyResults(PipelineResultTypes.Error, PipelineResultTypes.Output);

            ps.AddCommand("Tee-Object").AddParameter("Variable", "__LareLastOutput");
            ps.AddCommand("Out-Default");

            ps.Invoke();
        }
        catch (RuntimeException rex)
        {
            // Errori di runtime non catturati dal merge (raro, ma possibile per
            // errori "terminating" sollevati fuori dalla pipeline stessa).
            host.UI.WriteErrorLine(rex.Message);
        }
        catch (Exception ex) when (ex is ParseException or PipelineStoppedException)
        {
            // ParseException: errore di sintassi nello script (fallisce prima ancora
            //   di iniziare l'esecuzione, quindi non passa dal merge Error->Output).
            // PipelineStoppedException: la pipeline è stata fermata da ps.Stop()
            //   in risposta a un Ctrl+C.
            if (ex is PipelineStoppedException)
            {
                lock (ConsoleLock)
                {
                    Console.ForegroundColor = ConsoleColor.DarkGray;
                    Console.WriteLine("[LARE] comando interrotto (Ctrl+C).");
                    Console.ResetColor();
                }
            }
            else
            {
                host.UI.WriteErrorLine(ex.Message);
            }
        }
        finally
        {
            _currentPipeline = null;
        }

        int count = ReadAndClearObjectCount(runspace);
        lock (ConsoleLock)
        {
            Console.ForegroundColor = ConsoleColor.DarkGray;
            Console.WriteLine("[LARE] oggetti prodotti: " + count);
            Console.ResetColor();
        }
    }

    /// <summary>
    /// Legge il numero di oggetti accumulati da Tee-Object nella variabile
    /// __LareLastOutput (escludendo gli ErrorRecord, vedi commento sopra) e la
    /// rimuove, il tutto con un piccolissimo script PowerShell: usare il
    /// linguaggio stesso per questa operazione ci evita di dover indovinare/usare
    /// API interne meno pubbliche per leggere variabili di sessione da .NET.
    /// </summary>
    private static int ReadAndClearObjectCount(Runspace runspace)
    {
        const string script = """
            $__c = 0
            if ($null -ne $__LareLastOutput) {
                $items = @($__LareLastOutput)
                $__c = @($items | Where-Object { $_ -isnot [System.Management.Automation.ErrorRecord] }).Count
            }
            Remove-Variable -Name __LareLastOutput -ErrorAction SilentlyContinue
            $__c
            """;
        try
        {
            using var ps = PowerShell.Create();
            ps.Runspace = runspace;
            Collection<PSObject> result = ps.AddScript(script).Invoke();
            if (result.Count > 0 && int.TryParse(result[0].BaseObject?.ToString(), out int n))
            {
                return n;
            }
        }
        catch
        {
            // Se anche questo piccolo script fallisce, ripieghiamo su 0: non è
            // critico, è solo un contatore informativo per lo spike.
        }

        return 0;
    }

    /// <summary>
    /// Dot-sourcing dei profili utente, se esistono. In un host ospitato $PROFILE
    /// NON viene popolato automaticamente dal motore (lo fa ConsoleHost stesso):
    /// dobbiamo calcolare i percorsi a mano, seguendo la stessa convenzione di
    /// PowerShell (cartella Documents\PowerShell, nome file basato su $Host.Name
    /// per il profilo "CurrentHost").
    /// </summary>
    private static void DotSourceProfiles(Runspace runspace, LareHost host)
    {
        string psDocs = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "PowerShell");
        string allHostsProfile = Path.Combine(psDocs, "profile.ps1");
        string currentHostProfile = Path.Combine(psDocs, host.Name + "_profile.ps1");

        foreach (string profilePath in new[] { allHostsProfile, currentHostProfile })
        {
            if (!File.Exists(profilePath))
            {
                // Sulla macchina di sviluppo questi file non esistono: è normale,
                // lo segnaliamo solo come informazione, non come errore.
                lock (ConsoleLock)
                {
                    Console.ForegroundColor = ConsoleColor.DarkGray;
                    Console.WriteLine("[LARE] profilo non trovato (ok): " + profilePath);
                    Console.ResetColor();
                }
                continue;
            }

            try
            {
                using var ps = PowerShell.Create();
                ps.Runspace = runspace;
                // ". <percorso>" è l'operatore di dot-sourcing di PowerShell: esegue lo
                // script nello scope corrente invece che in uno scope figlio, così le
                // funzioni/variabili definite nel profilo restano disponibili dopo.
                ps.AddScript(". '" + profilePath.Replace("'", "''") + "'").Invoke();
                lock (ConsoleLock)
                {
                    Console.WriteLine("[LARE] profilo caricato: " + profilePath);
                }
            }
            catch (Exception ex)
            {
                host.UI.WriteErrorLine("Errore nel caricare il profilo " + profilePath + ": " + ex.Message);
            }
        }
    }
}
