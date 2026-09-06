using System.Text;

namespace LareShell.Host;

/// <summary>
/// Registra il testo che il motore scrive tramite la <c>PSHostUserInterface</c> (Task 3,
/// LareHostUI) durante un <c>ExecInShell</c> con <c>capture: true</c> (spec §4.5): è l'output che
/// torna all'AI nell'<c>ExecResult</c>. Cap "200 KB testa+coda" (§9): oltre <see cref="MaxChars"/>
/// si tengono i primi <see cref="HeadChars"/> e gli ultimi <see cref="TailChars"/> caratteri, con un
/// marcatore in mezzo che dice quanti ne sono stati omessi. Contati in caratteri, non byte (ruling 6).
/// Thread-safe: i Write* possono arrivare da thread del motore diversi dal REPL.
/// </summary>
internal sealed class OutputRecorder
{
    public const int MaxChars = 200 * 1024;
    private const int HeadChars = MaxChars / 2;
    private const int TailChars = MaxChars / 2;

    private readonly object _lock = new();
    private StringBuilder? _head;
    private StringBuilder? _tail;
    private long _omitted;

    public bool IsRecording
    {
        get { lock (_lock) { return _head is not null; } }
    }

    public void Begin()
    {
        lock (_lock)
        {
            _head = new StringBuilder();
            _tail = new StringBuilder();
            _omitted = 0;
        }
    }

    public void Append(string text)
    {
        if (string.IsNullOrEmpty(text))
        {
            return;
        }

        lock (_lock)
        {
            if (_head is null || _tail is null)
            {
                return;   // non stiamo registrando (capture:false o comando dell'utente)
            }

            // Prima si riempie la testa; il resto va in coda.
            int room = HeadChars - _head.Length;
            if (room > 0)
            {
                int take = Math.Min(room, text.Length);
                _head.Append(text, 0, take);
                if (take == text.Length)
                {
                    return;
                }

                text = text[take..];
            }

            _tail.Append(text);

            // La coda cresce fino a 2×TailChars, poi si taglia a TailChars: ammortizza il costo
            // del Remove (O(n)) invece di tagliare a ogni Append.
            if (_tail.Length > TailChars * 2)
            {
                int drop = _tail.Length - TailChars;
                _tail.Remove(0, drop);
                _omitted += drop;
            }
        }
    }

    /// <summary>Chiude la registrazione e restituisce il testo (con marcatore se troncato).</summary>
    public string End()
    {
        lock (_lock)
        {
            if (_head is null || _tail is null)
            {
                return string.Empty;
            }

            string result;
            if (_tail.Length > TailChars)
            {
                int drop = _tail.Length - TailChars;
                _tail.Remove(0, drop);
                _omitted += drop;
            }

            if (_omitted == 0)
            {
                result = _head.ToString() + _tail;
            }
            else
            {
                result = _head + Environment.NewLine
                       + "…[output troncato: " + _omitted + " caratteri omessi]…" + Environment.NewLine
                       + _tail;
            }

            _head = null;
            _tail = null;
            _omitted = 0;
            return result;
        }
    }
}
