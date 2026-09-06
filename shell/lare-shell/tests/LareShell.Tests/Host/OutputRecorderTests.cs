using LareShell.Host;
using Xunit;

namespace LareShell.Tests.Host;

public class OutputRecorderTests
{
    [Fact]
    public void Senza_Begin_le_scritture_sono_ignorate_e_End_da_stringa_vuota()
    {
        var r = new OutputRecorder();
        r.Append("perso");
        Assert.False(r.IsRecording);
        Assert.Equal(string.Empty, r.End());
    }

    [Fact]
    public void Begin_Append_End_restituisce_il_testo_e_azzera()
    {
        var r = new OutputRecorder();
        r.Begin();
        Assert.True(r.IsRecording);
        r.Append("uno ");
        r.Append("due");
        Assert.Equal("uno due", r.End());
        Assert.False(r.IsRecording);
        r.Begin();
        Assert.Equal(string.Empty, r.End());
    }

    [Fact]
    public void Sotto_il_cap_nessun_marcatore()
    {
        var r = new OutputRecorder();
        r.Begin();
        r.Append(new string('a', OutputRecorder.MaxChars));
        string s = r.End();
        Assert.Equal(OutputRecorder.MaxChars, s.Length);
        Assert.DoesNotContain("troncato", s);
    }

    [Fact]
    public void Oltre_il_cap_tiene_testa_e_coda_con_marcatore_e_conteggio()
    {
        // spec §4.5/§9: "cap 200 KB testa+coda", "troncato testa+coda con marcatore"
        var r = new OutputRecorder();
        r.Begin();
        int half = OutputRecorder.MaxChars / 2;
        r.Append(new string('h', half));           // testa
        r.Append(new string('m', 100_000));        // in mezzo: da omettere
        r.Append(new string('t', half));           // coda
        string s = r.End();
        Assert.StartsWith(new string('h', half), s);
        Assert.EndsWith(new string('t', half), s);
        Assert.Contains("[output troncato: 100000 caratteri omessi]", s);
    }

    [Fact]
    public void Molti_Append_piccoli_oltre_il_cap_non_esplodono_in_memoria()
    {
        var r = new OutputRecorder();
        r.Begin();
        for (int i = 0; i < 300_000; i++)
        {
            r.Append("xy");   // 600.000 caratteri, ~3× il cap
        }

        string s = r.End();
        Assert.True(s.Length <= OutputRecorder.MaxChars + 200, "lunghezza " + s.Length);
        Assert.Contains("caratteri omessi", s);
    }
}
