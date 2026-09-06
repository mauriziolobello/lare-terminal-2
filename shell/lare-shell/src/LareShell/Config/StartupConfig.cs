using System.Text.Json;
using System.Text.Json.Nodes;

namespace LareShell.Config;

/// <summary>
/// Lettura di <c>startup.json</c> (spec §6.3) — stesso file e stesso schema del crate Rust
/// <c>startup-config</c>; qui servono solo <c>ws_port</c> e <c>autostart</c>. Tutti i campi sono
/// opzionali con questi default; file assente = default; file malformato = default + un avviso
/// (mai un'eccezione: spec §9 "startup.json malformato → log + default, mai panic").
/// Le proprietà sono <c>init</c>: l'oggetto è immutabile dopo la costruzione.
/// </summary>
internal sealed class StartupConfig
{
    public int WsPort { get; init; } = 7331;
    public bool AutostartOrchestrator { get; init; } = true;
    public bool AutostartUi { get; init; } = true;
    public IReadOnlyList<string> Warnings { get; init; } = Array.Empty<string>();

    public static StartupConfig Load(string configDir)
    {
        string path = Path.Combine(configDir, "startup.json");
        if (!File.Exists(path))
        {
            return new StartupConfig();
        }

        try
        {
            return Parse(File.ReadAllText(path));
        }
        // UnauthorizedAccessException NON deriva da IOException (sono due rami distinti della
        // gerarchia): un permesso negato sul file va catturato esplicitamente, altrimenti
        // risalirebbe fino a Main e farebbe cadere la host, contro la regola "mai un'eccezione".
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return new StartupConfig { Warnings = new[] { "startup.json non leggibile (" + ex.Message + "): uso i default" } };
        }
    }

    public static StartupConfig Parse(string? json)
    {
        if (json is null)
        {
            return new StartupConfig();
        }

        try
        {
            // JsonNode invece di classi [JsonProperty]: lo schema completo ha molti campi che qui
            // non servono (paths, ai_model, log) e non vogliamo replicarlo — leggiamo solo le chiavi note.
            JsonObject root = JsonNode.Parse(json) as JsonObject
                ?? throw new JsonException("la radice non è un oggetto JSON");
            JsonObject? autostart = root["autostart"] as JsonObject;
            return new StartupConfig
            {
                WsPort = root["ws_port"]?.GetValue<int>() ?? 7331,
                AutostartOrchestrator = autostart?["orchestrator"]?.GetValue<bool>() ?? true,
                AutostartUi = autostart?["ui"]?.GetValue<bool>() ?? true,
            };
        }
        catch (Exception ex) when (ex is JsonException or InvalidOperationException or FormatException)
        {
            return new StartupConfig { Warnings = new[] { "startup.json malformato (" + ex.Message + "): uso i default" } };
        }
    }
}
