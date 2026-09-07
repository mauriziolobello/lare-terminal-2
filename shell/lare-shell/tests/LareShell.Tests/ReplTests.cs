using System.Text.RegularExpressions;
using LareShell;
using Xunit;

namespace LareShell.Tests;

public class SlashLineTests
{
    [Theory]
    [InlineData("/ping", true)]
    [InlineData("   /ping", true)]          // spazi iniziali (spec §10)
    [InlineData("/", true)]                 // "/" solo
    [InlineData("/ \"ciao\"", true)]        // / "x" ≡ /ai "x"
    [InlineData("Get-Date", false)]
    [InlineData("", false)]
    [InlineData("   ", false)]
    [InlineData("C:/x", false)]
    [InlineData("dir / ", false)]
    public void IsSlash_riconosce_le_righe_da_intercettare(string raw, bool expected)
    {
        Assert.Equal(expected, SlashLine.IsSlash(raw));
    }

    [Fact]
    public void Normalize_toglie_gli_spazi_ai_bordi_e_basta()
    {
        Assert.Equal("/ai \"x  y\"", SlashLine.Normalize("  /ai \"x  y\"  "));
    }
}

public class OscTests
{
    [Fact]
    public void Intercept_costruisce_ESC_9001_lare_intercept_riga_ESC_backslash()
    {
        // spec §4.7: ESC ] 9001 ; lare ; intercept ; <riga> ESC \
        string s = Osc.Intercept("/ping");
        Assert.Equal("\u001b]9001;lare;intercept;/ping\u001b\\", s);
        Assert.Equal((char)0x1B, Osc.Esc);
    }

    [Fact]
    public void Intercept_rimuove_i_caratteri_di_controllo_dal_payload()
    {
        // Un ESC o un a-capo nel testo spezzerebbe la sequenza: il payload resta pulito.
        string s = Osc.Intercept("/ai \"a\u001bb\nc\"");
        Assert.Equal("\u001b]9001;lare;intercept;/ai \"abc\"\u001b\\", s);
    }

    [Fact]
    public void Intercept_tronca_payload_oltre_4096_caratteri()
    {
        // F5c (revisione finale piano 2b): una riga patologica (incollata, non digitata) non deve
        // produrre una OSC chilometrica. Il payload va troncato a 4096 caratteri, qualunque sia la
        // lunghezza della riga originale, il resto della sequenza (ESC iniziale/finale) resta intatto.
        string riga = new string('x', 5000);
        string s = Osc.Intercept(riga);
        string atteso = Osc.Esc + "]9001;lare;intercept;" + new string('x', 4096) + Osc.Esc + "\\";
        Assert.Equal(atteso, s);
    }
}

public class SourceScanTests
{
    [Fact]
    public void Nessun_sorgente_della_host_costruisce_ESC_con_un_escape_di_stringa()
    {
        // spec §10: "OSC 9001 costruito senza \x (test che fallisce se ricompare un "\x1b")".
        // Bug "\x greedy" dello spike 1: "\x1b]9001…" legge le cifre esadecimali seguenti come
        // parte del codice. L'unico modo ammesso è (char)0x1B (Osc.Esc).
        string src = Path.Combine(TestPaths.RepoRoot(), "shell", "lare-shell", "src", "LareShell");
        var offenders = new List<string>();
        var pattern = new Regex(@"\\x1[bB]|\\u001[bB]|\\e\b");
        foreach (string file in Directory.EnumerateFiles(src, "*.cs", SearchOption.AllDirectories))
        {
            foreach ((string line, int n) in File.ReadLines(file).Select((l, i) => (l, i + 1)))
            {
                if (pattern.IsMatch(line))
                {
                    offenders.Add(Path.GetRelativePath(src, file) + ":" + n + ": " + line.Trim());
                }
            }
        }

        Assert.True(offenders.Count == 0, "ESC scritto come escape di stringa in:\n" + string.Join("\n", offenders));
    }
}
