using Xunit;

namespace LareShell.Tests.Shell;

/// <summary>
/// I test che aprono una runspace vera girano in serie: Runspace.DefaultRunspace è per thread e
/// RunspaceSession.Open modifica il PSModulePath del processo. Tutto il resto resta parallelo.
/// </summary>
[CollectionDefinition("runspace", DisableParallelization = true)]
public class RunspaceCollection
{
}
