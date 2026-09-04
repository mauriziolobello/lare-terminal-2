using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Management.Automation.Runspaces;

namespace LareShellSpike;

/// <summary>
/// Repl contiene il ciclo read-eval-print-loop vero e proprio: crea la runspace
/// ospitata (con PSReadLine importato), legge le righe di comando esattamente come
/// fa pwsh.exe (invocando la funzione PSConsoleHostReadLine), intercetta le righe
/// che iniziano con "/" senza eseguirle in PowerShell, ed esegue tutto il resto
/// nella runspace mostrando l'output "normale" via Out-Default (vedi il commento
/// su Repl.Execute per il perché qui NON c'è nessun cmdlet di conteggio oggetti
/// interposto nella pipeline, a differenza di --selftest).
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
            // ORDINE IMPORTANTE (bug osservato: il prompt iniziale veniva stampato
            // SOPRA la barra in basso, "prompt> ...  /help  /config" sulla stessa
            // riga): la VT processing è già abilitata dal costruttore di StatusBar
            // (poco sopra); qui statusBar.Start() applica la scroll region, disegna
            // le due barre e clampa il cursore dentro la regione (riga 2..H-1) PRIMA
            // di stampare qualunque cosa. Solo DOPO stampiamo banner/profili/prompt,
            // così finiscono tutti dentro la regione gestita, mai sopra la barra.
            statusBar.Start();

            lock (ConsoleLock)
            {
                Console.WriteLine("Lare Terminal 2.0 - spike host PowerShell");
                Console.WriteLine("PSReadLine disponibile: " + (psReadLineAvailable ? "si" : "NO (fallback a Console.ReadLine)"));
                Console.WriteLine("Righe che iniziano con '/' vengono intercettate, non eseguite. '/exit' esce.");
                Console.WriteLine();
            }

            DotSourceProfiles(runspace, host);

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
                    // Ridisegno dopo ogni comando "intercettato": non strettamente
                    // necessario quanto dopo un comando PowerShell (qui non tocchiamo
                    // lo schermo intero), ma economico e coerente con "dopo ogni
                    // comando eseguito" — e copre il caso in cui un futuro comando /
                    // faccia output più corposo.
                    statusBar.Redraw();

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

                Execute(runspace, host, line);

                // Bug osservato: dopo Clear-Host la barra in basso spariva (coperto
                // ora anche da LareRawUI.SetBufferContents, vedi StatusBar.Current),
                // e più in generale qualunque comando che scrive molto output può
                // aver disturbato le due righe fisse. Ridisegniamo sempre, qui, dopo
                // OGNI comando PowerShell eseguito, non solo dopo Clear-Host.
                statusBar.Redraw();
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
    /// Esegue la riga NON intercettata nella runspace ospitata e mostra l'output
    /// "normale" via Out-Default, con la stessa formattazione a colori che vedresti
    /// in pwsh.exe: pipeline = AddScript(riga) | Out-Default, ESATTAMENTE come fa
    /// ConsoleHost reale, senza nessun cmdlet interposto fra lo script dell'utente
    /// e Out-Default.
    ///
    /// STORIA (round 2 dello spike, bug confermato — Fix 6): la prima versione di
    /// questo metodo inseriva un Tee-Object -Variable in mezzo alla pipeline per
    /// mostrare anche un conteggio "[LARE] oggetti prodotti: N" (Tee-Object duplica
    /// ogni oggetto: una copia a Out-Default, l'altra in una variabile di sessione
    /// letta/rimossa da una seconda, piccola invocazione PowerShell). Sembrava
    /// innocuo ("Out-Default resta l'ultimo comando, l'utente vede lo stesso
    /// output"), ma non lo è per i comandi NATIVI (un .exe, non un cmdlet):
    /// NativeCommandProcessor decide se un processo nativo eredita direttamente i
    /// device standard della console (stdout/stderr = la console vera) oppure se
    /// il suo output va rediretto su una pipe .NET, in base a SE è l'ULTIMO comando
    /// della pipeline. Con Tee-Object di mezzo, un comando nativo non è più
    /// l'ultimo: il suo stdout finisce su una pipe, non sulla console. Sintomo
    /// osservato: lanciando `python` (il suo REPL interattivo), il modulo Python
    /// _pyrepl (windows_console.py, getheightwidth) chiama
    /// GetConsoleScreenBufferInfo su un handle che non è più quello della console
    /// reale (è una pipe), la chiamata fallisce, e python va in loop stampando
    /// l'eccezione finché non si interrompe con Ctrl+C. Fix: NESSUN cmdlet fra lo
    /// script dell'utente e Out-Default. Il conteggio oggetti resta disponibile
    /// SOLO in modalità --selftest (vedi Program.cs/SelfTest.Run), dove il conteggio
    /// è fatto direttamente lato .NET su PowerShell.Invoke() senza toccare affatto
    /// la pipeline interattiva.
    /// </summary>
    private static void Execute(Runspace runspace, LareHost host, string line)
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
            // silenziosamente in ps.Streams.Error senza mai essere mostrati. Questo
            // NON introduce un cmdlet nella pipeline (è solo redirezione di stream),
            // quindi non ha l'effetto collaterale di Tee-Object spiegato sopra.
            ps.Commands.Commands[0].MergeMyResults(PipelineResultTypes.Error, PipelineResultTypes.Output);

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
