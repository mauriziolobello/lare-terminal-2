namespace LareShell.Host;

/// <summary>
/// Confine d'astrazione sulle console mode Win32 (GetConsoleMode/SetConsoleMode): LareHost ne
/// dipende per NotifyBegin/EndApplication, e nei test una finta registra le mode senza console.
/// È lo stesso ruolo di un trait Rust con un fake al seam.
/// </summary>
internal interface IConsoleModes
{
    IntPtr GetHandle(int stdHandle);

    bool TryGetMode(IntPtr handle, out uint mode);

    bool TrySetMode(IntPtr handle, uint mode);
}

/// <summary>Implementazione vera: delega alle P/Invoke di <see cref="ConsoleModes"/>.</summary>
internal sealed class Win32ConsoleModes : IConsoleModes
{
    public static Win32ConsoleModes Instance { get; } = new();

    public IntPtr GetHandle(int stdHandle) => ConsoleModes.GetHandle(stdHandle);

    public bool TryGetMode(IntPtr handle, out uint mode) => ConsoleModes.TryGetMode(handle, out mode);

    public bool TrySetMode(IntPtr handle, uint mode) => ConsoleModes.TrySetMode(handle, mode);
}
