namespace LareShell.Tests;

/// <summary>
/// Trova la radice del repo risalendo da <c>AppContext.BaseDirectory</c> (bin/Debug/net10.0 del
/// progetto di test) fino alla cartella che contiene <c>Cargo.toml</c>: i test che leggono file
/// veri del repo (startup.json di Test Run, sorgenti di src/) non devono dipendere dalla cwd.
/// </summary>
internal static class TestPaths
{
    public static string RepoRoot()
    {
        DirectoryInfo? dir = new(AppContext.BaseDirectory);
        while (dir is not null)
        {
            if (File.Exists(Path.Combine(dir.FullName, "Cargo.toml")))
            {
                return dir.FullName;
            }

            dir = dir.Parent;
        }

        throw new InvalidOperationException("radice del repo (Cargo.toml) non trovata sopra " + AppContext.BaseDirectory);
    }
}
