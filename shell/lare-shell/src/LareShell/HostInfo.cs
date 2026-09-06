namespace LareShell;

/// <summary>
/// Identità della host in UN solo posto: la usano <c>PSHost.Name</c>/<c>Version</c> (Task 3),
/// <c>Hello.version</c> (Task 2, mostrata da <c>/ping</c>), il nome del profilo
/// <c>LareShell_profile.ps1</c> (Task 4) e il banner del REPL (Task 7).
/// </summary>
internal static class HostInfo
{
    public const string Name = "LareShell";
    public const string Version = "2.0.0";
}
