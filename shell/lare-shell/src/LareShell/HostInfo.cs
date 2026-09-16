using System.Reflection;

namespace LareShell;

/// <summary>
/// Identità della host in UN solo posto: la usano <c>PSHost.Name</c>/<c>Version</c> (Task 3),
/// <c>Hello.version</c> (Task 2, mostrata da <c>/ping</c>), il nome del profilo
/// <c>LareShell_profile.ps1</c> (Task 4) e il banner del REPL (Task 7).
/// </summary>
internal static class HostInfo
{
    public const string Name = "LareShell";

    /// <summary>
    /// Versione letta a runtime dall'assembly — MSBuild la genera dal solo
    /// <c>&lt;Version&gt;</c> dichiarato in <c>LareShell.csproj</c> (via
    /// <c>AssemblyVersionAttribute</c>), senza bisogno di scriverla altrove. Prima di questo fix
    /// (2026-09-17, segnalato da Maurizio: il banner del REPL mostrava "2.0.0" nonostante il
    /// progetto fosse già a 2.0.1) era una <c>const string</c> hardcoded qui — un secondo posto,
    /// oltre al <c>.csproj</c>, da tenere sincronizzato a mano, e nessuno lo aveva più fatto dal
    /// bump iniziale. "UN solo posto" (vedi il commento della classe) ora lo è davvero: il
    /// <c>.csproj</c>, non più duplicato in codice.
    ///
    /// <c>AssemblyName.Version</c> (non <c>AssemblyInformationalVersionAttribute</c>) apposta:
    /// l'informational version, in un repo git, include automaticamente un suffisso
    /// "+&lt;commit-sha&gt;" (SourceRevisionId) che <see cref="System.Version"/> non sa parsare —
    /// <c>LareHost.cs</c> costruisce proprio un <see cref="System.Version"/> da questa stringa.
    /// <c>AssemblyName.Version</c> resta sempre puramente numerico; <c>ToString(3)</c> lo riporta
    /// a "major.minor.build" (es. "2.0.1"), scartando il quarto componente ".0" che MSBuild
    /// aggiunge da solo quando il <c>.csproj</c> ne dichiara solo tre.
    /// </summary>
    public static readonly string Version =
        Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "0.0.0";
}
