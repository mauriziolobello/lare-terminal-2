# Piano 2b — Host C# `lare-shell` (modalità B in Windows Terminal)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `Test Run\shell\lare-shell.exe`, aperto nudo in una scheda di Windows Terminal (profilo
"Lare Terminal"), è una sessione PowerShell completa (PSReadLine, profili, prompt dell'utente) in cui
ogni riga `/…` va all'orchestratore sul canale shell del piano 2a: `/ai "…"` chiede `[Y/n]` nel
terminale, esegue i comandi dell'AI **nel runspace dell'utente** (`ExecInShell`/`ExecResult`, cwd
persistente) e l'esito compare in una finestra di `ui.exe`; `/ping`, `/help`, `/config` funzionano;
uno slash ignoto è muto. Se orchestratore o `ui.exe` mancano, la host li avvia (§6.4).

**Architecture:** un progetto .NET 10 `shell/lare-shell/` (exe `lare-shell`, `Microsoft.PowerShell.SDK`
7.6.5) con tre strati: `Config` (`--config-dir`, `startup.json`, `token`, log su file), `Protocol`
(`Wire` = JSON snake_case ↔ record C#; `OrchestratorClient` = `ClientWebSocket` con loop di ricezione
su thread proprio che **accoda** in un `Channel<ServerMessage>`), `Shell`/`Host` (host `PSHost` copiate
dallo spike + un **registratore** dei `Write*` della `PSHostUserInterface` per la cattura; `Executor`
= pipeline `script | [ForEach-Object] | Out-Default`; `SlashTurn` = ciclo di un turno sul **thread del
REPL**, che consuma il canale: gate `[Y/n]`, exec, `Done`/`Error`). Il REPL è **un solo thread
sincrono** che possiede runspace e console; il thread del socket non tocca mai né l'una né l'altra.
Autostart di `orchestrator.exe`/`ui.exe` con `Launcher` (processi senza console ereditata). Test xUnit
con un **server WS finto in-process** (`TcpListener` + upgrade a mano, mai `HttpListener`).

**Tech Stack:** C# 13 / .NET SDK 10.0.400 (`net10.0`), `Microsoft.PowerShell.SDK` 7.6.5, pwsh 7.6.5
installato (PSReadLine viene dai suoi moduli), xUnit 2.9.3 + `Microsoft.NET.Test.Sdk` 17.14.1 +
`xunit.runner.visualstudio` 3.1.4 (versioni già ripristinate su questa macchina), `System.Text.Json`
(`JsonNode`), `System.Net.WebSockets`, `System.Threading.Channels`. PowerShell 7 per gli script.

**Spec:** `Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` — §2.3 (modalità B,
gara all'avvio), §4.1–4.7 (protocollo, sequenza del turno, gate, vincoli di esecuzione, `capture`,
cwd, OSC 9001), §6.1–6.4 (configurazione, autostart), §7 (`Test Run\shell\`, prerequisiti, publish),
§8 (sicurezza), §9 (errori), §10 (test C#), §13 (verifiche). Contratti per la host (a)–(d) in
`crates/protocol/IMPLEMENTATION.md` §"Contratti per la host (piano 2b)". Spike di riferimento:
`spikes/lare-shell-host/` (codice) e `Docs/i18n/ita/spikes/2026-09-05-lare-shell-host.md` (esito).
Client di sviluppo che imita la host: `scripts/dev/shell-client.mjs`.

## Global Constraints

Copiate dallo spec e dai contratti; ogni task le include implicitamente.

- **Un solo thread tocca runspace e console** (§4.4): "`ExecInShell` e il prompt `[Y/n]` vengono
  marshalizzati sul thread del REPL … mai invocati dal thread del socket. Il thread WS accoda; il
  REPL consuma." Nel codice: il loop di ricezione scrive SOLO in un `Channel<ServerMessage>`; il REPL
  legge il canale con ponti bloccanti (`.GetAwaiter().GetResult()`) — nessun `async` nel REPL
  (`Runspace.DefaultRunspace` è thread-static, le continuazioni cambierebbero thread).
- **Gate prima di tutto** (§8): "La host esegue **solo** ciò che arriva come `ExecInShell` sulla
  connessione WS autenticata, dopo il gate; mai testo da altre vie (l'OSC 9001 è solo in uscita)."
  Un `ExecInShell` con `turn_id` diverso dal turno corrente NON viene eseguito (scartato con log).
- **Contratti per la host** (`crates/protocol/IMPLEMENTATION.md`): **(a)** un solo turno AI alla
  volta per connessione — la host non manda un secondo `Command` prima di `Done`/`Error` del
  precedente (garantito dal REPL bloccato); **(b)** `Command.id` UNICO per connessione (GUID);
  **(c)** il turno finisce al PRIMO `Done`/`Error` — i messaggi successivi con quell'`id` vengono
  scartati; **(d)** `ExecResult.turn_id` = il `turn_id` ricevuto nell'`ExecInShell` corrispondente,
  mai uno proprio.
- **`capture`** (§4.5): `true` → pipeline `<cmd> | ForEach-Object { $_ } | Out-Default` (un cmdlet
  in mezzo: anche i programmi nativi passano dalla pipe), `output` = tutto ciò che la
  `PSHostUserInterface` ha scritto (stdout+stderr fusi), cap **200 KB testa+coda** con marcatore;
  `false` → `<cmd> | Out-Default` pura (console attaccata), `output` vuoto, restano `exit_code` e `cwd`.
- **Ctrl+C** (§4.4): in attesa del turno → `CancelCommand{id}`, una riga, prompt; durante un
  `ExecInShell` → `PowerShell.Stop()` **e** `CancelCommand` — mai un `ExecResult` parziale.
- **cwd** (§4.6): `Command.cwd` = `$PWD` del runspace all'invio (`SessionStateProxy.Path.
  CurrentLocation.ProviderPath`, mai `Directory.GetCurrentDirectory()`); ogni `ExecResult.cwd` è la
  cwd dopo il comando; `Hello.cwd` alla connessione.
- **Configurazione** (§6.1, D6): `--config-dir <path>` altrimenti `<cartella dell'exe>\..\Configuration\`
  (per `lare-shell.exe` in `shell\`). **Nessuna variabile d'ambiente `LARE_*`, `LOCALAPPDATA`,
  `APPDATA` come sorgente di configurazione Lare.** I figli ricevono `--config-dir` per argomento.
  (`PSModulePath` e `PATH` sono variabili di PowerShell/Windows, non configurazione Lare: leggerle
  per trovare pwsh è lecito; `%LOCALAPPDATA%` nello script del profilo WT è dove Windows Terminal
  cerca i fragment — integrazione con un'app terza, come Dropbox in §6.1.)
- **Sicurezza** (§8): WS solo `ws://127.0.0.1:<ws_port>/`, token letto da `<config-dir>\token`.
- **Execution policy** (§4.4, §13): non forzata in-process. La host spedisce accanto all'exe un
  `powershell.config.json` = `{"Microsoft.PowerShell:ExecutionPolicy":"RemoteSigned"}`: è il file
  che PowerShell 7 legge per lo scope LocalMachine (`$PSHOME\powershell.config.json`, dove `$PSHOME`
  di un'app che ospita il motore è la cartella dell'app). `Set-ExecutionPolicy -Scope LocalMachine`
  dentro Lare riscrive quel file: "risolta come ConsoleHost".
- **Profili** (§4.4): dot-source, in quest'ordine, di `~\Documents\PowerShell\profile.ps1`,
  `~\Documents\PowerShell\Microsoft.PowerShell_profile.ps1` (quello di pwsh, così alias/oh-my-posh
  appaiono), `~\Documents\PowerShell\LareShell_profile.ps1`; `$PROFILE` = quest'ultimo, con le 4
  NoteProperty di pwsh. I profili AllUsers (in `$PSHOME` di pwsh) NON vengono caricati (debito).
- **PSReadLine** (§7): il NuGet non lo include; viene da `<cartella di pwsh>\Modules`. Su questa
  macchina quella cartella NON è nel `PSModulePath` di macchina/utente (verificato 2026-09-06): la
  host la antepone al `PSModulePath` **del processo** prima di aprire la runspace. Se pwsh non si
  trova: riga leggibile + fallback `Console.ReadLine`. stdin rediretto → sempre `Console.ReadLine`.
- **OSC 9001** (§4.7): `ESC ] 9001 ; lare ; intercept ; <riga> ESC \` con `ESC = (char)0x1B`; un
  test fallisce se nei sorgenti di `src/` ricompare un escape `\x1b` o `\u001b`. Niente canale
  "titolo" (lo spike lo usava come riserva: in una scheda WT farebbe lampeggiare il titolo).
- **Autostart** (§6.4): WS non raggiungibile → se `autostart.orchestrator`, avvia
  `<radice deploy>\orchestrator.exe --config-dir <dir>` e ritenta per **5 s**; poi, se
  `autostart.ui` e nessun processo con `MainModule.FileName == <radice deploy>\ui.exe`, avvia
  `ui.exe --config-dir <dir>`. Processi figli **senza console ereditata** (`UseShellExecute=true`,
  finestra nascosta per l'orchestratore): un Ctrl+C nella shell non li abbatte.
- **Errori** (§9): orchestratore irraggiungibile → riga d'errore, la shell **resta usabile**; WS
  cade durante un turno → riga d'errore, prompt; riconnessione al prossimo `/…` (con autostart).
- **Versioni/doc**: componente nuovo, `lare-shell` **2.0.0** (`shell/lare-shell/CHANGELOG.md` +
  `IMPLEMENTATION.md`, HANDOFF, ADR-019 nel task di release). Codice, test e commenti in italiano,
  didattici (§11). TDD con RED reale: ogni step "verifica che fallisca" va eseguito davvero.
- **Modelli**: implementer Sonnet (Haiku dove il task è trascrizione pura: 1, 8 — il Task 3 copia
  quattro classi con modifiche mirate: Sonnet), reviewer Sonnet, Task 5 (concorrenza) e review finale su Opus.

## Struttura dei file

```
shell/lare-shell/
├── LareShell.sln
├── CHANGELOG.md · IMPLEMENTATION.md                         (Task 9)
├── src/LareShell/
│   ├── LareShell.csproj            exe "lare-shell", net10.0, PowerShell.SDK 7.6.5, Version 2.0.0
│   ├── powershell.config.json      execution policy LocalMachine = RemoteSigned (copiato accanto all'exe)
│   ├── HostInfo.cs                 Name/Version della host (un solo posto)
│   ├── Program.cs                  Main: argomenti, config, log, runspace, client, REPL; --selftest
│   ├── Repl.cs                     ciclo prompt→riga→(slash | PowerShell), Ctrl+C, riconnessione
│   ├── SlashLine.cs · Osc.cs       riconoscimento "/…" · OSC 9001
│   ├── Config/CliArgs.cs · ConfigDir.cs · StartupConfig.cs · TokenFile.cs · HostLog.cs
│   ├── Protocol/Wire.cs            record ServerMessage + Parse + costruttori dei messaggi client
│   ├── Protocol/OrchestratorClient.cs   ClientWebSocket, hello/server_info, loop → Channel
│   ├── Host/IConsoleModes.cs · ConsoleModes.cs (P/Invoke) · LareHost.cs · LareHostUI.cs · LareRawUI.cs
│   ├── Host/OutputRecorder.cs      registra i Write* (cap 200 KB testa+coda)
│   ├── Shell/PwshLocator.cs        cartella di pwsh (PATH, registro, Program Files)
│   ├── Shell/RunspaceSession.cs    runspace ospitata + PSReadLine + prompt + ReadLine
│   ├── Shell/ProfileLoader.cs      $PROFILE e dot-source dei tre profili
│   ├── Shell/Executor.cs           IExecutor: Run(command, capture) → ExecOutcome; RunInteractive
│   ├── Shell/Gate.cs               IGate, GateAnswer, ConsoleGate ([Y/n] con polling dei tasti)
│   ├── Shell/SlashTurn.cs          ciclo di un turno sul thread del REPL
│   └── Shell/Launcher.cs           IProcessStarter, ProcessStarter, Launcher (autostart + retry 5 s)
└── tests/LareShell.Tests/
    ├── LareShell.Tests.csproj      xunit 2.9.3, ProjectReference a src (InternalsVisibleTo)
    ├── TestPaths.cs                radice del repo (cerca Cargo.toml risalendo)
    ├── Config/ConfigTests.cs · Protocol/WireTests.cs · Protocol/FakeOrchestrator.cs
    ├── Protocol/OrchestratorClientTests.cs · Host/LareHostTests.cs · Host/OutputRecorderTests.cs
    ├── Shell/PwshLocatorTests.cs · Shell/RunspaceSessionTests.cs · Shell/ProfileLoaderTests.cs
    ├── Shell/ExecutorTests.cs · Shell/SlashTurnTests.cs · Shell/LauncherTests.cs
    └── ReplTests.cs                SlashLine, Osc, scansione dei sorgenti per "\x1b"
Test Run/install-wt-profile.ps1 · uninstall-wt-profile.ps1                       (Task 8)
deploy_test_run.ps1 (+ dotnet publish → Test Run\shell\) · .gitignore (Test Run/shell/)
```

I test che aprono una runspace vera (`RunspaceSessionTests`, `ProfileLoaderTests`, `ExecutorTests`)
stanno nella collection xUnit `"runspace"` (serializzati: `Runspace.DefaultRunspace` è per thread e
il `PSModulePath` del processo è condiviso). Tutto il resto gira in parallelo.

## Ruling presi in questo piano (da riferire a Maurizio)

1. **Server finto su `TcpListener` + upgrade a mano**, non `HttpListener`: il probe .NET di questa
   sessione ha lasciato una registrazione http.sys appesa che ha fatto fallire il rerun ("conflicts
   with an existing registration"); `HttpListener` è anche sensibile alle URL ACL per i non-admin.
2. **Riconnessione "on demand"**: nessun task di riconnessione in background; alla prossima riga
   `/…` la host prova a riconnettersi (con autostart e finestra di 5 s). Soddisfa §9 ("riconnessione
   automatica con backoff") dal punto di vista dell'utente con molta meno concorrenza.
3. **`exit_code`** = `0` se `$?` del comando è vero; altrimenti `$LASTEXITCODE` se ≠ 0, altrimenti
   `1`. `$?` è catturato **in coda allo stesso script** (`$global:__lare_ok = $?` come ultima
   istruzione: letto da una pipeline separata rifletterebbe la pipeline esterna, che riesce
   sempre); `$LASTEXITCODE` è azzerato prima del comando, così un valore ≠ 0 dopo è suo. Per i
   comandi digitati dall'utente NON si tocca nulla (così `$?` resta corretto per il prompt/oh-my-posh).
4. **Processi figli con `UseShellExecute = true`** (nessun handle ereditato) e `WindowStyle =
   Hidden` per `orchestrator.exe` (app console): equivalente pratico di `DETACHED_PROCESS` in
   .NET; `ui.exe` (app GUI) con `WindowStyle = Normal`. Emendamento a §6.4.
5. **`ToolConfirmRequest` non porta il `turn_id`** (il suo `id` è opaco, v1): durante un turno ogni
   richiesta di conferma è attribuita al turno corrente (contratto (a)); a inizio turno i messaggi
   rimasti in coda da turni chiusi vengono scartati. Il prompt `[Y/n]` si abbandona da solo se in
   coda c'è già `Done`/`Error` del turno (timeout 180 s dell'orchestratore).
6. **Cap dell'output in caratteri** (200·1024), non in byte.
7. **Log della host** `<config-dir>\logs\lare-shell.log` senza rotazione (debito, §6.2 chiede
   rotazione giornaliera 7 file).
8. **`ui.exe` avviata** dopo la prima connessione riuscita e ricontrollata prima di ogni turno slash
   (self-heal): se l'utente l'ha chiusa, un `/…` la riapre.
9. **`deploy_test_run.ps1`** pubblica la host di default (`dotnet publish` framework-dependent
   `win-x64`); `-SkipShell` per saltarla.

---

### Task 0: Soluzione .NET, configurazione (`--config-dir`, `startup.json`, `token`), log su file

**Files:**
- Create: `shell/lare-shell/LareShell.sln`
- Create: `shell/lare-shell/src/LareShell/LareShell.csproj`
- Create: `shell/lare-shell/src/LareShell/powershell.config.json`
- Create: `shell/lare-shell/src/LareShell/HostInfo.cs`
- Create: `shell/lare-shell/src/LareShell/Program.cs` (provvisorio: stampa la config risolta; il Task 7 lo completa)
- Create: `shell/lare-shell/src/LareShell/Config/CliArgs.cs`, `ConfigDir.cs`, `StartupConfig.cs`, `TokenFile.cs`, `HostLog.cs`
- Create: `shell/lare-shell/tests/LareShell.Tests/LareShell.Tests.csproj`, `TestPaths.cs`, `Config/ConfigTests.cs`
- Modify: `.gitignore` (aggiungi `Test Run/shell/` sotto le righe `Test Run/**/*.exe`/`*.dll`)

**Interfaces:**
- Produces: `CliArgs.Parse(IReadOnlyList<string>) → CliArgs(string? ConfigDir, string? SessionId, bool SelfTest)`;
  `ConfigDir.Resolve(string? cliValue, string exeDir) → string` e `ConfigDir.DeployRoot(string configDir) → string`;
  `StartupConfig.Load(string configDir)` / `StartupConfig.Parse(string? json)` con `WsPort`, `AutostartOrchestrator`,
  `AutostartUi`, `Warnings`; `TokenFile.Read(string configDir) → string?`; `HostLog.Open(string configDir)` con
  `Info/Warn/Debug(string)`; `HostInfo.Name = "LareShell"`, `HostInfo.Version = "2.0.0"`; `TestPaths.RepoRoot()` per i test.

- [ ] **Step 1: Crea soluzione e progetti**

Da `shell/lare-shell/` (crea la cartella). Non usare `dotnet new console`/`xunit` per i csproj: scrivili
a mano come sotto (il template xunit di .NET 10 aggiunge `coverlet` e `Using`, non servono).

```powershell
New-Item -ItemType Directory -Force shell/lare-shell/src/LareShell/Config, shell/lare-shell/tests/LareShell.Tests/Config | Out-Null
Set-Location shell/lare-shell
dotnet new sln -n LareShell --format sln
```

Se il flag `--format` venisse rifiutato, riportalo nel report e usa il file che il template produce
(`.slnx`): NON riscrivere i comandi del piano, che citano `LareShell.sln`, prima di averlo detto.

`src/LareShell/LareShell.csproj`:

```xml
<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <!-- Nome dell'eseguibile (lare-shell.exe, spec §7) e namespace radice del codice. -->
    <AssemblyName>lare-shell</AssemblyName>
    <RootNamespace>LareShell</RootNamespace>
    <Version>2.0.0</Version>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <!-- Il NuGet Microsoft.PowerShell.SDK porta le risorse localizzate di decine di lingue:
         teniamo solo l'inglese nell'output (≈ -30 MB nel publish). -->
    <SatelliteResourceLanguages>en</SatelliteResourceLanguages>
  </PropertyGroup>

  <ItemGroup>
    <PackageReference Include="Microsoft.PowerShell.SDK" Version="7.6.5" />
  </ItemGroup>

  <ItemGroup>
    <!-- Execution policy di scope LocalMachine (spec §4.4/§13, ADR-019): PowerShell 7 la legge da
         $PSHOME\powershell.config.json, e $PSHOME di un'app che ospita il motore è la cartella
         dell'app. Copiato accanto all'exe (e, transitivamente, nell'output dei test). -->
    <None Include="powershell.config.json" CopyToOutputDirectory="PreserveNewest" />
  </ItemGroup>

  <ItemGroup>
    <!-- I tipi sono `internal`: i test li vedono grazie a questo attributo di assembly. -->
    <InternalsVisibleTo Include="LareShell.Tests" />
  </ItemGroup>

</Project>
```

`src/LareShell/powershell.config.json`:

```json
{
  "Microsoft.PowerShell:ExecutionPolicy": "RemoteSigned"
}
```

`tests/LareShell.Tests/LareShell.Tests.csproj`:

```xml
<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <IsPackable>false</IsPackable>
    <IsTestProject>true</IsTestProject>
    <SatelliteResourceLanguages>en</SatelliteResourceLanguages>
  </PropertyGroup>

  <ItemGroup>
    <PackageReference Include="Microsoft.NET.Test.Sdk" Version="17.14.1" />
    <PackageReference Include="xunit" Version="2.9.3" />
    <PackageReference Include="xunit.runner.visualstudio" Version="3.1.4" />
  </ItemGroup>

  <ItemGroup>
    <ProjectReference Include="..\..\src\LareShell\LareShell.csproj" />
  </ItemGroup>

</Project>
```

```powershell
dotnet sln add src/LareShell/LareShell.csproj tests/LareShell.Tests/LareShell.Tests.csproj
```

`src/LareShell/HostInfo.cs`:

```csharp
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
```

`src/LareShell/Program.cs` (provvisorio — il Task 7 lo sostituisce):

```csharp
using LareShell.Config;

namespace LareShell;

internal static class Program
{
    private static int Main(string[] args)
    {
        // UTF-8 in output: senza, le lettere accentate dei nostri messaggi si corrompono
        // su console con codepage non-UTF8 (lezione dello spike). Try/catch: stdout rediretto.
        try { Console.OutputEncoding = System.Text.Encoding.UTF8; } catch { /* ignorabile */ }

        CliArgs cli = CliArgs.Parse(args);
        string configDir = ConfigDir.Resolve(cli.ConfigDir, AppContext.BaseDirectory);
        Console.WriteLine("lare-shell " + HostInfo.Version + " — config: " + configDir);
        return 0;
    }
}
```

- [ ] **Step 2: Scrivi i test della configurazione (RED)**

`tests/LareShell.Tests/TestPaths.cs`:

```csharp
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
```

`tests/LareShell.Tests/Config/ConfigTests.cs`:

```csharp
using LareShell.Config;
using Xunit;

namespace LareShell.Tests.Config;

public class CliArgsTests
{
    [Fact]
    public void Parse_legge_config_dir_session_e_selftest()
    {
        CliArgs a = CliArgs.Parse(new[] { "--config-dir", @"D:\cfg", "--session", "abc", "--selftest" });
        Assert.Equal(@"D:\cfg", a.ConfigDir);
        Assert.Equal("abc", a.SessionId);
        Assert.True(a.SelfTest);
    }

    [Fact]
    public void Parse_senza_argomenti_da_tutti_null_e_false()
    {
        CliArgs a = CliArgs.Parse(Array.Empty<string>());
        Assert.Null(a.ConfigDir);
        Assert.Null(a.SessionId);
        Assert.False(a.SelfTest);
    }

    [Fact]
    public void Parse_ignora_config_dir_senza_valore_in_coda()
    {
        CliArgs a = CliArgs.Parse(new[] { "--config-dir" });
        Assert.Null(a.ConfigDir);
    }
}

public class ConfigDirTests
{
    [Fact]
    public void Resolve_senza_flag_usa_la_cartella_Configuration_accanto_alla_cartella_dell_exe()
    {
        // spec §6.1: lare-shell.exe vive in <deploy>\shell\ → <deploy>\Configuration\
        string dir = ConfigDir.Resolve(null, @"C:\Lare\shell\");
        Assert.Equal(@"C:\Lare\Configuration", dir);
    }

    [Fact]
    public void Resolve_con_flag_usa_il_valore_dato()
    {
        Assert.Equal(@"D:\cfg", ConfigDir.Resolve(@"D:\cfg", @"C:\Lare\shell\"));
    }

    [Fact]
    public void DeployRoot_e_il_padre_della_cartella_di_configurazione()
    {
        Assert.Equal(@"C:\Lare", ConfigDir.DeployRoot(@"C:\Lare\Configuration"));
    }
}

public class StartupConfigTests
{
    [Fact]
    public void Parse_null_da_i_default()
    {
        StartupConfig c = StartupConfig.Parse(null);
        Assert.Equal(7331, c.WsPort);
        Assert.True(c.AutostartOrchestrator);
        Assert.True(c.AutostartUi);
        Assert.Empty(c.Warnings);
    }

    [Fact]
    public void Parse_legge_ws_port_e_autostart()
    {
        StartupConfig c = StartupConfig.Parse("{ \"ws_port\": 8000, \"autostart\": { \"orchestrator\": false, \"ui\": false } }");
        Assert.Equal(8000, c.WsPort);
        Assert.False(c.AutostartOrchestrator);
        Assert.False(c.AutostartUi);
    }

    [Fact]
    public void Parse_malformato_da_i_default_con_un_avviso_mai_eccezione()
    {
        // spec §9: "startup.json malformato → log + default, mai panic"
        StartupConfig c = StartupConfig.Parse("{ ws_port: ");
        Assert.Equal(7331, c.WsPort);
        Assert.Single(c.Warnings);
        Assert.Contains("startup.json", c.Warnings[0]);
    }

    [Fact]
    public void Load_legge_lo_startup_json_committato_in_Test_Run()
    {
        // Lo stesso file che legge l'orchestratore (crate startup-config): se lo schema cambia
        // di là, questo test se ne accorge di qua.
        string configDir = Path.Combine(TestPaths.RepoRoot(), "Test Run", "Configuration");
        StartupConfig c = StartupConfig.Load(configDir);
        Assert.Equal(7331, c.WsPort);
        Assert.True(c.AutostartOrchestrator);
        Assert.True(c.AutostartUi);
        Assert.Empty(c.Warnings);
    }

    [Fact]
    public void Load_senza_file_da_i_default()
    {
        string dir = Path.Combine(Path.GetTempPath(), "lare-cfg-" + Guid.NewGuid().ToString("N"));
        StartupConfig c = StartupConfig.Load(dir);
        Assert.Equal(7331, c.WsPort);
        Assert.Empty(c.Warnings);
    }
}

public class TokenFileTests
{
    [Fact]
    public void Read_ritorna_il_token_trimmato_o_null_se_manca()
    {
        string dir = Path.Combine(Path.GetTempPath(), "lare-tok-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        try
        {
            Assert.Null(TokenFile.Read(dir));
            File.WriteAllText(Path.Combine(dir, "token"), "abc123\r\n");
            Assert.Equal("abc123", TokenFile.Read(dir));
            File.WriteAllText(Path.Combine(dir, "token"), "   ");
            Assert.Null(TokenFile.Read(dir));
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }
}

public class HostLogTests
{
    [Fact]
    public void Open_crea_logs_e_scrive_righe_con_livello()
    {
        string dir = Path.Combine(Path.GetTempPath(), "lare-log-" + Guid.NewGuid().ToString("N"));
        try
        {
            HostLog log = HostLog.Open(dir);
            log.Info("ciao");
            log.Warn("attenzione");
            string text = File.ReadAllText(Path.Combine(dir, "logs", "lare-shell.log"));
            Assert.Contains("INFO ciao", text);
            Assert.Contains("WARN attenzione", text);
        }
        finally
        {
            if (Directory.Exists(dir)) Directory.Delete(dir, recursive: true);
        }
    }

    [Fact]
    public void Open_su_percorso_non_scrivibile_non_lancia_e_le_scritture_sono_no_op()
    {
        // Un percorso impossibile (carattere non valido): il log deve degradare in silenzio,
        // mai far cadere la shell per un problema di log (spec §9: la shell resta usabile).
        HostLog log = HostLog.Open("Z:\\<>|\0impossibile");
        log.Info("niente");
    }
}
```

- [ ] **Step 3: Verifica che fallisca (RED)**

```powershell
dotnet test shell/lare-shell/LareShell.sln
```

Atteso: errori di compilazione `CS0246` (tipo o spazio dei nomi `CliArgs`/`ConfigDir`/… non trovato).
Riporta le prime righe nel report.

- [ ] **Step 4: Implementa `Config/`**

`src/LareShell/Config/CliArgs.cs`:

```csharp
namespace LareShell.Config;

/// <summary>
/// Argomenti della riga di comando della host (spec §2.3/§6.1):
/// <c>--config-dir &lt;path&gt;</c> (cartella di configurazione), <c>--session &lt;id&gt;</c>
/// (passato da ui.exe in modalità A: lega la connessione shell alla finestra terminale),
/// <c>--selftest</c> (controlli non interattivi, exit code 0/1). Argomenti sconosciuti ignorati.
/// Un <c>record</c> immutabile: è un valore, non un oggetto con comportamento.
/// </summary>
internal sealed record CliArgs(string? ConfigDir, string? SessionId, bool SelfTest)
{
    public static CliArgs Parse(IReadOnlyList<string> args)
    {
        string? configDir = null;
        string? sessionId = null;
        bool selfTest = false;

        for (int i = 0; i < args.Count; i++)
        {
            switch (args[i])
            {
                // `when i + 1 < args.Count`: il flag in coda senza valore viene ignorato,
                // non fa cadere il processo.
                case "--config-dir" when i + 1 < args.Count:
                    configDir = args[++i];
                    break;
                case "--session" when i + 1 < args.Count:
                    sessionId = args[++i];
                    break;
                case "--selftest":
                    selfTest = true;
                    break;
            }
        }

        return new CliArgs(configDir, sessionId, selfTest);
    }
}
```

`src/LareShell/Config/ConfigDir.cs`:

```csharp
namespace LareShell.Config;

/// <summary>
/// Regola unica di risoluzione della cartella di configurazione (spec §6.1, ADR-017):
/// 1. <c>--config-dir &lt;path&gt;</c> se presente;
/// 2. altrimenti <c>&lt;cartella dell'exe&gt;\..\Configuration\</c> — lare-shell.exe vive in
///    <c>&lt;deploy&gt;\shell\</c>, quindi la Configuration è quella del deploy, accanto a shell\.
/// NESSUNA variabile d'ambiente (LARE_*, LOCALAPPDATA, APPDATA) viene letta: è la regola D6.
/// </summary>
internal static class ConfigDir
{
    public static string Resolve(string? cliValue, string exeDir)
    {
        string dir = string.IsNullOrWhiteSpace(cliValue)
            ? Path.Combine(exeDir, "..", "Configuration")
            : cliValue;
        // GetFullPath normalizza i ".." e la barra finale (C:\Lare\shell\..\Configuration → C:\Lare\Configuration).
        return Path.GetFullPath(dir);
    }

    /// <summary>Radice del deploy = cartella padre di Configuration\ (spec §6.3): lì stanno
    /// orchestrator.exe e ui.exe che la host avvia in autostart (§6.4).</summary>
    public static string DeployRoot(string configDir) => Path.GetFullPath(Path.Combine(configDir, ".."));
}
```

`src/LareShell/Config/StartupConfig.cs`:

```csharp
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
        catch (IOException ex)
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
```

`src/LareShell/Config/TokenFile.cs`:

```csharp
namespace LareShell.Config;

/// <summary>
/// Il token del WS (spec §8: "WS solo su 127.0.0.1 + token su file") sta in
/// <c>&lt;config-dir&gt;\token</c>, generato dall'orchestratore al primo avvio. Prima che
/// l'orchestratore sia partito il file può non esistere: <c>null</c>, non un'eccezione — il
/// Launcher (Task 6) rilegge il file a ogni tentativo di connessione.
/// </summary>
internal static class TokenFile
{
    public static string? Read(string configDir)
    {
        string path = Path.Combine(configDir, "token");
        try
        {
            if (!File.Exists(path))
            {
                return null;
            }

            string token = File.ReadAllText(path).Trim();
            return token.Length == 0 ? null : token;
        }
        catch (IOException)
        {
            return null;
        }
    }
}
```

`src/LareShell/Config/HostLog.cs`:

```csharp
namespace LareShell.Config;

/// <summary>
/// Log su file della host: <c>&lt;config-dir&gt;\logs\lare-shell.log</c> (spec §6.2). Best-effort:
/// se la cartella non si può creare o il file non si può scrivere, ogni chiamata è un no-op —
/// un problema di log non deve mai far cadere la shell (§9). Senza rotazione (debito, HANDOFF).
/// Thread-safe (lock): scrivono sia il thread del REPL sia il loop di ricezione del WS.
/// </summary>
internal sealed class HostLog
{
    private readonly string? _path;
    private readonly object _lock = new();

    private HostLog(string? path) => _path = path;

    /// <summary>Log che scarta tutto (test, o quando la cartella non è scrivibile).</summary>
    public static HostLog Null { get; } = new(null);

    public static HostLog Open(string configDir)
    {
        try
        {
            string dir = Path.Combine(configDir, "logs");
            Directory.CreateDirectory(dir);
            return new HostLog(Path.Combine(dir, "lare-shell.log"));
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            return Null;
        }
    }

    public void Info(string message) => Write("INFO", message);

    public void Warn(string message) => Write("WARN", message);

    public void Debug(string message) => Write("DEBUG", message);

    private void Write(string level, string message)
    {
        if (_path is null)
        {
            return;
        }

        lock (_lock)
        {
            try
            {
                File.AppendAllText(_path, DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss") + " " + level + " " + message + Environment.NewLine);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // Best-effort: vedi il commento di classe.
            }
        }
    }
}
```

- [ ] **Step 5: Verifica che passi (GREEN)**

```powershell
dotnet test shell/lare-shell/LareShell.sln
```

Atteso: `Superati! - Non superati: 0, Superati: 14` (o `Passed!` in inglese). Poi
`dotnet run --project shell/lare-shell/src/LareShell -- --config-dir "Test Run\Configuration"` stampa
`lare-shell 2.0.0 — config: …\Test Run\Configuration`.

- [ ] **Step 6: `.gitignore` e commit**

In `.gitignore`, dopo la riga `Test Run/**/*.dll`, aggiungi:

```
# Output di `dotnet publish` della host (lare-shell.exe + DLL + Modules\ + .json del motore)
Test Run/shell/
```

```powershell
git add .gitignore shell/lare-shell
git commit -m "feat(lare-shell): soluzione .NET, configurazione (--config-dir, startup.json, token) e log su file"
```

### Task 1: `Protocol/Wire.cs` — messaggi del canale shell come record C#, parse e serializzazione

**Files:**
- Create: `shell/lare-shell/src/LareShell/Protocol/Wire.cs`
- Test: `shell/lare-shell/tests/LareShell.Tests/Protocol/WireTests.cs`

**Interfaces:**
- Consumes: niente (solo `System.Text.Json`).
- Produces (usati dai Task 2, 5): `abstract record ServerMessage` con le varianti `ServerInfo(string Version,
  string AiProvider)`, `Chunk(string Id, string Content)`, `Done(string Id, int? ExitCode)`, `TurnError(string Id,
  string Code, string Message)`, `ToolConfirmRequest(string Id, string Commands)`, `ExecInShell(string TurnId,
  string ExecId, string Command, bool Capture)`, `Heartbeat(string Id)`, `Pong(long Ts)`, `Unknown(string Type)`,
  `Disconnected(string Reason)` (sintetico: lo genera il client, mai il filo); `WireException`;
  `Wire.Parse(string json) → ServerMessage`; costruttori `Wire.Hello(token, sessionId, cwd, version)`,
  `Wire.Command(id, input, cwd)`, `Wire.ToolConfirmResponse(id, accept)`, `Wire.ExecResult(turnId, execId,
  exitCode, output, cwd)`, `Wire.CancelCommand(id)`, `Wire.Ping(ts)` → `string` JSON.

Forme sul filo (crate `protocol`, `#[serde(tag = "type", rename_all = "snake_case")]`, spec §4.1):

```jsonc
// host → orchestratore
{"type":"hello","token":"…","role":"shell","session_id":"…","cwd":"C:\\…","version":"2.0.0"}
{"type":"command","id":"…","input":"/ai \"…\"","input_mode":"keyboard","command_type":"auto","cwd":"C:\\…","web_search":false}
{"type":"tool_confirm_response","id":"…","accept":true}
{"type":"exec_result","turn_id":"…","exec_id":"…","exit_code":0,"output":"…","cwd":"C:\\…"}
{"type":"cancel_command","id":"…"}
{"type":"ping","ts":123}
// orchestratore → host
{"type":"server_info","version":"…","ai_provider":"…","capabilities":[…]}
{"type":"chunk","id":"…","content":"…"}   {"type":"done","id":"…","exit_code":null}
{"type":"error","id":"…","code":"routing_error","message":"…"}
{"type":"tool_confirm_request","id":"<opaco>","commands":"…"}
{"type":"exec_in_shell","turn_id":"…","exec_id":"…","command":"…","capture":true}
{"type":"heartbeat","id":"…"}   {"type":"pong","ts":123}
```

- [ ] **Step 1: Scrivi i test (RED)**

`tests/LareShell.Tests/Protocol/WireTests.cs`:

```csharp
using System.Text.Json.Nodes;
using LareShell.Protocol;
using Xunit;

namespace LareShell.Tests.Protocol;

public class WireParseTests
{
    [Fact]
    public void Parse_server_info()
    {
        ServerMessage m = Wire.Parse("{\"type\":\"server_info\",\"version\":\"2.1.0\",\"ai_provider\":\"anthropic\",\"capabilities\":[\"a\"]}");
        ServerInfo info = Assert.IsType<ServerInfo>(m);
        Assert.Equal("2.1.0", info.Version);
        Assert.Equal("anthropic", info.AiProvider);
    }

    [Fact]
    public void Parse_chunk_done_error()
    {
        Chunk c = Assert.IsType<Chunk>(Wire.Parse("{\"type\":\"chunk\",\"id\":\"t1\",\"content\":\"ciao\"}"));
        Assert.Equal(("t1", "ciao"), (c.Id, c.Content));

        Done d = Assert.IsType<Done>(Wire.Parse("{\"type\":\"done\",\"id\":\"t1\",\"exit_code\":null}"));
        Assert.Equal("t1", d.Id);
        Assert.Null(d.ExitCode);

        Done d2 = Assert.IsType<Done>(Wire.Parse("{\"type\":\"done\",\"id\":\"t1\",\"exit_code\":3}"));
        Assert.Equal(3, d2.ExitCode);

        TurnError e = Assert.IsType<TurnError>(Wire.Parse("{\"type\":\"error\",\"id\":\"t1\",\"code\":\"routing_error\",\"message\":\"boom\"}"));
        Assert.Equal(("t1", "routing_error", "boom"), (e.Id, e.Code, e.Message));
    }

    [Fact]
    public void Parse_tool_confirm_request_ed_exec_in_shell()
    {
        ToolConfirmRequest r = Assert.IsType<ToolConfirmRequest>(Wire.Parse("{\"type\":\"tool_confirm_request\",\"id\":\"g1\",\"commands\":\"Get-Date\"}"));
        Assert.Equal(("g1", "Get-Date"), (r.Id, r.Commands));

        ExecInShell x = Assert.IsType<ExecInShell>(Wire.Parse("{\"type\":\"exec_in_shell\",\"turn_id\":\"t1\",\"exec_id\":\"e1\",\"command\":\"dir\",\"capture\":false}"));
        Assert.Equal(("t1", "e1", "dir", false), (x.TurnId, x.ExecId, x.Command, x.Capture));
    }

    [Fact]
    public void Parse_heartbeat_e_pong()
    {
        Assert.Equal("t1", Assert.IsType<Heartbeat>(Wire.Parse("{\"type\":\"heartbeat\",\"id\":\"t1\"}")).Id);
        Assert.Equal(42L, Assert.IsType<Pong>(Wire.Parse("{\"type\":\"pong\",\"ts\":42}")).Ts);
    }

    [Fact]
    public void Parse_tipo_sconosciuto_da_Unknown_con_il_nome_del_tipo()
    {
        // Un messaggio v1 che la shell non gestisce (es. cwd, open_window) non deve far cadere
        // il loop di ricezione: viene consegnato come Unknown e il consumatore lo ignora.
        Unknown u = Assert.IsType<Unknown>(Wire.Parse("{\"type\":\"open_window\",\"kind\":\"markdown\"}"));
        Assert.Equal("open_window", u.Type);
    }

    [Fact]
    public void Parse_ignora_campi_extra()
    {
        Chunk c = Assert.IsType<Chunk>(Wire.Parse("{\"type\":\"chunk\",\"id\":\"t1\",\"content\":\"x\",\"extra\":1}"));
        Assert.Equal("x", c.Content);
    }

    [Theory]
    [InlineData("non json")]
    [InlineData("{\"id\":\"t1\"}")]                       // manca type
    [InlineData("{\"type\":\"chunk\",\"id\":\"t1\"}")]     // manca content
    [InlineData("[1,2]")]                                  // non un oggetto
    public void Parse_invalido_lancia_WireException(string json)
    {
        Assert.Throws<WireException>(() => Wire.Parse(json));
    }
}

public class WireSerializeTests
{
    private static JsonObject Obj(string json) => (JsonObject)JsonNode.Parse(json)!;

    [Fact]
    public void Hello_ha_role_shell_session_cwd_e_version()
    {
        JsonObject o = Obj(Wire.Hello("tok", "s1", @"C:\x", "2.0.0"));
        Assert.Equal("hello", (string?)o["type"]);
        Assert.Equal("tok", (string?)o["token"]);
        Assert.Equal("shell", (string?)o["role"]);
        Assert.Equal("s1", (string?)o["session_id"]);
        Assert.Equal(@"C:\x", (string?)o["cwd"]);
        Assert.Equal("2.0.0", (string?)o["version"]);
        // `channel` assente o null: la connessione shell non chiede un canale esterno.
        Assert.True(o["channel"] is null);
    }

    [Fact]
    public void Command_ha_i_campi_obbligatori_v1_con_i_valori_fissi()
    {
        JsonObject o = Obj(Wire.Command("id1", "/ai \"x\"", @"C:\x"));
        Assert.Equal("command", (string?)o["type"]);
        Assert.Equal("id1", (string?)o["id"]);
        Assert.Equal("/ai \"x\"", (string?)o["input"]);
        // input_mode e command_type NON hanno default lato Rust: vanno sempre inviati.
        Assert.Equal("keyboard", (string?)o["input_mode"]);
        Assert.Equal("auto", (string?)o["command_type"]);
        Assert.Equal(@"C:\x", (string?)o["cwd"]);
        Assert.False((bool?)o["web_search"]);
    }

    [Fact]
    public void ToolConfirmResponse_ExecResult_CancelCommand_Ping()
    {
        JsonObject a = Obj(Wire.ToolConfirmResponse("g1", true));
        Assert.Equal("tool_confirm_response", (string?)a["type"]);
        Assert.Equal("g1", (string?)a["id"]);
        Assert.True((bool?)a["accept"]);

        JsonObject r = Obj(Wire.ExecResult("t1", "e1", 2, "out", @"C:\y"));
        Assert.Equal("exec_result", (string?)r["type"]);
        Assert.Equal("t1", (string?)r["turn_id"]);
        Assert.Equal("e1", (string?)r["exec_id"]);
        Assert.Equal(2, (int?)r["exit_code"]);
        Assert.Equal("out", (string?)r["output"]);
        Assert.Equal(@"C:\y", (string?)r["cwd"]);

        JsonObject c = Obj(Wire.CancelCommand("id1"));
        Assert.Equal("cancel_command", (string?)c["type"]);
        Assert.Equal("id1", (string?)c["id"]);

        JsonObject p = Obj(Wire.Ping(7));
        Assert.Equal("ping", (string?)p["type"]);
        Assert.Equal(7L, (long?)p["ts"]);
    }

    [Fact]
    public void Serializzazione_preserva_unicode_e_ritorni_a_capo()
    {
        JsonObject r = Obj(Wire.ExecResult("t", "e", 0, "è\r\nà", @"C:\"));
        Assert.Equal("è\r\nà", (string?)r["output"]);
    }
}
```

- [ ] **Step 2: Verifica che fallisca (RED)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Wire"
```

Atteso: `CS0246` su `Wire`/`ServerMessage`.

- [ ] **Step 3: Implementa `Wire.cs`**

```csharp
using System.Text.Json;
using System.Text.Json.Nodes;

namespace LareShell.Protocol;

// ── Messaggi orchestratore → host ─────────────────────────────────────────────
// Un record per variante, con una classe base astratta: è l'equivalente C# dell'enum
// `ServerMsg` di Rust (crates/protocol). Il consumatore fa `switch (msg) { case Chunk c: … }`
// — pattern matching sui tipi, come il `match` di Rust. Solo le varianti che la shell
// gestisce hanno un record proprio; tutto il resto diventa `Unknown`.

internal abstract record ServerMessage;

internal sealed record ServerInfo(string Version, string AiProvider) : ServerMessage;

internal sealed record Chunk(string Id, string Content) : ServerMessage;

internal sealed record Done(string Id, int? ExitCode) : ServerMessage;

/// <summary>`error` sul filo. Si chiama TurnError e non Error per non confondersi con
/// <c>System.Exception</c>/la parola chiave dei log.</summary>
internal sealed record TurnError(string Id, string Code, string Message) : ServerMessage;

/// <summary>Gate ADR-007: <c>Id</c> è opaco (non è il turn_id), <c>Commands</c> può avere più righe.</summary>
internal sealed record ToolConfirmRequest(string Id, string Commands) : ServerMessage;

internal sealed record ExecInShell(string TurnId, string ExecId, string Command, bool Capture) : ServerMessage;

internal sealed record Heartbeat(string Id) : ServerMessage;

internal sealed record Pong(long Ts) : ServerMessage;

/// <summary>Tipo non gestito dalla shell (es. messaggi v1 per la ui): il loop lo consegna e va avanti.</summary>
internal sealed record Unknown(string Type) : ServerMessage;

/// <summary>Sintetico: lo accoda <c>OrchestratorClient</c> quando la connessione cade, così il
/// consumatore (thread del REPL) lo scopre leggendo il canale come ogni altro messaggio.</summary>
internal sealed record Disconnected(string Reason) : ServerMessage;

internal sealed class WireException : Exception
{
    public WireException(string message) : base(message) { }

    public WireException(string message, Exception inner) : base(message, inner) { }
}

/// <summary>
/// Traduzione fra il JSON del protocollo (snake_case, campo discriminante <c>type</c>) e i
/// record C#. Nessuna rete qui: funzioni pure, testabili senza socket.
/// </summary>
internal static class Wire
{
    public static ServerMessage Parse(string json)
    {
        JsonObject obj;
        try
        {
            obj = JsonNode.Parse(json) as JsonObject ?? throw new WireException("il messaggio non è un oggetto JSON");
        }
        catch (JsonException ex)
        {
            throw new WireException("JSON non valido: " + ex.Message, ex);
        }

        string type = Str(obj, "type");
        return type switch
        {
            "server_info" => new ServerInfo(Str(obj, "version"), Str(obj, "ai_provider")),
            "chunk" => new Chunk(Str(obj, "id"), Str(obj, "content")),
            "done" => new Done(Str(obj, "id"), obj["exit_code"]?.GetValue<int>()),
            "error" => new TurnError(Str(obj, "id"), Str(obj, "code"), Str(obj, "message")),
            "tool_confirm_request" => new ToolConfirmRequest(Str(obj, "id"), Str(obj, "commands")),
            "exec_in_shell" => new ExecInShell(Str(obj, "turn_id"), Str(obj, "exec_id"), Str(obj, "command"), Bool(obj, "capture")),
            "heartbeat" => new Heartbeat(Str(obj, "id")),
            "pong" => new Pong(obj["ts"]?.GetValue<long>() ?? 0),
            _ => new Unknown(type),
        };
    }

    // ── Costruttori dei messaggi host → orchestratore ───────────────────────
    // Tipi anonimi con i nomi snake_case scritti a mano: il JSON che ne esce è esattamente
    // quello che serde si aspetta; non serve una naming policy né classi dedicate.

    /// <summary><c>version</c> è la SOLA versione ("2.0.0", HostInfo.Version): la riga di /ping la
    /// stampa come <c>| lare-shell | 2.0.0 | …</c> (orchestrator/ping.rs), il nome lo mette lui.</summary>
    public static string Hello(string token, string sessionId, string cwd, string version) =>
        JsonSerializer.Serialize(new { type = "hello", token, role = "shell", session_id = sessionId, cwd, version });

    /// <summary><c>input_mode</c>/<c>command_type</c> sono obbligatori lato Rust (nessun
    /// <c>#[serde(default)]</c>): fissi a <c>keyboard</c>/<c>auto</c> — la shell non ha voce né
    /// classificazione OS/NL (il pre-router della shell decide dal testo). <c>web_search</c> false:
    /// per la shell vale la casella di /config (piano 2a).</summary>
    public static string Command(string id, string input, string cwd) =>
        JsonSerializer.Serialize(new { type = "command", id, input, input_mode = "keyboard", command_type = "auto", cwd, web_search = false });

    public static string ToolConfirmResponse(string id, bool accept) =>
        JsonSerializer.Serialize(new { type = "tool_confirm_response", id, accept });

    public static string ExecResult(string turnId, string execId, int exitCode, string output, string cwd) =>
        JsonSerializer.Serialize(new { type = "exec_result", turn_id = turnId, exec_id = execId, exit_code = exitCode, output, cwd });

    public static string CancelCommand(string id) =>
        JsonSerializer.Serialize(new { type = "cancel_command", id });

    public static string Ping(long ts) =>
        JsonSerializer.Serialize(new { type = "ping", ts });

    // ── Helper di lettura: campo mancante o del tipo sbagliato → WireException ──

    private static string Str(JsonObject obj, string name)
    {
        JsonNode? node = obj[name];
        if (node is null)
        {
            throw new WireException("campo mancante: " + name);
        }

        try
        {
            return node.GetValue<string>();
        }
        catch (Exception ex) when (ex is InvalidOperationException or FormatException)
        {
            throw new WireException("campo " + name + " non è una stringa", ex);
        }
    }

    private static bool Bool(JsonObject obj, string name)
    {
        JsonNode? node = obj[name];
        if (node is null)
        {
            throw new WireException("campo mancante: " + name);
        }

        try
        {
            return node.GetValue<bool>();
        }
        catch (Exception ex) when (ex is InvalidOperationException or FormatException)
        {
            throw new WireException("campo " + name + " non è un booleano", ex);
        }
    }
}
```

- [ ] **Step 4: Verifica che passi (GREEN)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Wire"
```

Atteso: 14 test superati (10 di parse, di cui 4 dalla theory, + 4 di serializzazione).

- [ ] **Step 5: Commit**

```powershell
git add shell/lare-shell
git commit -m "feat(lare-shell): Wire — record dei messaggi del canale shell, parse e serializzazione snake_case"
```

### Task 2: `OrchestratorClient` + server WS finto in-process (`FakeOrchestrator`)

Primo task di rete, come chiede lo spec §13 ("protocollo host↔orchestratore: mai spikato — primo
task di implementazione con test contro server finto").

**Files:**
- Create: `shell/lare-shell/src/LareShell/Protocol/OrchestratorClient.cs`
- Create: `shell/lare-shell/tests/LareShell.Tests/Protocol/FakeOrchestrator.cs`
- Test: `shell/lare-shell/tests/LareShell.Tests/Protocol/OrchestratorClientTests.cs`

**Interfaces:**
- Consumes: `Wire`, i record `ServerMessage` (Task 1); `HostLog` (Task 0).
- Produces: `OrchestratorClient(Uri uri, Func<string?> tokenProvider, string sessionId, string version, HostLog log)`;
  `ChannelReader<ServerMessage> Incoming`; `bool IsConnected`; `string SessionId`;
  `Task<string?> ConnectAsync(string cwd, CancellationToken ct)` (null = connesso, altrimenti il motivo);
  `string? Connect(string cwd, TimeSpan timeout)` (ponte sincrono per il REPL);
  `Task<bool> SendAsync(string json, CancellationToken ct)`; `bool Send(string json)` (sincrono, timeout 5 s);
  `Dispose()`. Alla caduta della connessione accoda `Disconnected(reason)`.
- Produces (test): `FakeOrchestrator.Start(string? expectedToken = null)`; `Uri`; `Task<JsonObject> AcceptAsync(ct)`
  (upgrade + legge `hello`; token sbagliato → chiude senza `server_info`, come `ws.rs`); `Task<JsonObject> ReceiveAsync(ct)`;
  `Task SendAsync(string json, ct)`; `Task SendFragmentedAsync(string json, int splitAt, ct)`; `Task CloseAsync()`;
  `DisposeAsync()`.

- [ ] **Step 1: Scrivi il server finto**

`tests/LareShell.Tests/Protocol/FakeOrchestrator.cs`:

```csharp
using System.Net;
using System.Net.Sockets;
using System.Net.WebSockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;

namespace LareShell.Tests.Protocol;

/// <summary>
/// Orchestratore finto: un server WebSocket minimo in-process. NON usa HttpListener (registra
/// prefissi in http.sys che sopravvivono a un test caduto e richiedono URL ACL): TcpListener su
/// porta 0 + handshake HTTP di upgrade scritto a mano (RFC 6455 §4.2.2: risposta 101 con
/// Sec-WebSocket-Accept = base64(SHA1(key + GUID magico))) + WebSocket.CreateFromStream.
/// Gestisce UN client per volta, come serve ai test.
/// </summary>
internal sealed class FakeOrchestrator : IAsyncDisposable
{
    private const string WebSocketMagicGuid = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

    private readonly TcpListener _listener;
    private readonly string? _expectedToken;
    private TcpClient? _client;
    private WebSocket? _socket;

    private FakeOrchestrator(TcpListener listener, string? expectedToken)
    {
        _listener = listener;
        _expectedToken = expectedToken;
    }

    public int Port => ((IPEndPoint)_listener.LocalEndpoint).Port;

    public Uri Uri => new("ws://127.0.0.1:" + Port + "/");

    /// <param name="expectedToken">null = accetta qualunque token.</param>
    public static FakeOrchestrator Start(string? expectedToken = null)
    {
        var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        return new FakeOrchestrator(listener, expectedToken);
    }

    /// <summary>Accetta un client, fa l'upgrade, legge l'hello. Token sbagliato → chiude senza
    /// server_info (esattamente ciò che fa ws.rs); altrimenti manda server_info. Ritorna l'hello.</summary>
    public async Task<JsonObject> AcceptAsync(CancellationToken ct)
    {
        _client = await _listener.AcceptTcpClientAsync(ct);
        NetworkStream stream = _client.GetStream();
        _socket = await UpgradeAsync(stream, ct);

        JsonObject hello = await ReceiveAsync(ct);
        if (_expectedToken is not null && (string?)hello["token"] != _expectedToken)
        {
            await CloseAsync();
            return hello;
        }

        await SendAsync("{\"type\":\"server_info\",\"version\":\"fake\",\"ai_provider\":\"none\",\"capabilities\":[]}", ct);
        return hello;
    }

    public async Task<JsonObject> ReceiveAsync(CancellationToken ct)
    {
        WebSocket socket = _socket ?? throw new InvalidOperationException("nessun client accettato");
        var buffer = new byte[16 * 1024];
        using var ms = new MemoryStream();
        while (true)
        {
            WebSocketReceiveResult r = await socket.ReceiveAsync(buffer, ct);
            if (r.MessageType == WebSocketMessageType.Close)
            {
                throw new IOException("il client ha chiuso la connessione");
            }

            ms.Write(buffer, 0, r.Count);
            if (r.EndOfMessage)
            {
                break;
            }
        }

        string text = Encoding.UTF8.GetString(ms.ToArray());
        return (JsonObject)JsonNode.Parse(text)!;
    }

    public Task SendAsync(string json, CancellationToken ct)
    {
        WebSocket socket = _socket ?? throw new InvalidOperationException("nessun client accettato");
        return socket.SendAsync(Encoding.UTF8.GetBytes(json), WebSocketMessageType.Text, endOfMessage: true, ct);
    }

    /// <summary>Manda un messaggio di testo spezzato in DUE frame (endOfMessage false, poi true):
    /// serve a verificare che il client riassembli i frame fino a EndOfMessage.</summary>
    public async Task SendFragmentedAsync(string json, int splitAt, CancellationToken ct)
    {
        WebSocket socket = _socket ?? throw new InvalidOperationException("nessun client accettato");
        byte[] bytes = Encoding.UTF8.GetBytes(json);
        await socket.SendAsync(new ArraySegment<byte>(bytes, 0, splitAt), WebSocketMessageType.Text, endOfMessage: false, ct);
        await socket.SendAsync(new ArraySegment<byte>(bytes, splitAt, bytes.Length - splitAt), WebSocketMessageType.Text, endOfMessage: true, ct);
    }

    public async Task CloseAsync()
    {
        if (_socket is { State: WebSocketState.Open or WebSocketState.CloseReceived })
        {
            try
            {
                await _socket.CloseOutputAsync(WebSocketCloseStatus.NormalClosure, "bye", CancellationToken.None);
            }
            catch (Exception ex) when (ex is WebSocketException or IOException or ObjectDisposedException)
            {
                // Il client può aver già chiuso: irrilevante per i test.
            }
        }
    }

    public async ValueTask DisposeAsync()
    {
        await CloseAsync();
        _socket?.Dispose();
        _client?.Dispose();
        _listener.Stop();
    }

    private static async Task<WebSocket> UpgradeAsync(NetworkStream stream, CancellationToken ct)
    {
        // Legge la richiesta HTTP fino alla riga vuota che chiude le intestazioni.
        var buffer = new byte[8 * 1024];
        int total = 0;
        while (true)
        {
            int n = await stream.ReadAsync(buffer.AsMemory(total, buffer.Length - total), ct);
            if (n == 0)
            {
                throw new IOException("handshake troncato");
            }

            total += n;
            if (Encoding.ASCII.GetString(buffer, 0, total).Contains("\r\n\r\n", StringComparison.Ordinal))
            {
                break;
            }

            if (total == buffer.Length)
            {
                throw new IOException("intestazioni HTTP troppo lunghe");
            }
        }

        string request = Encoding.ASCII.GetString(buffer, 0, total);
        string keyLine = request.Split("\r\n").First(l => l.StartsWith("Sec-WebSocket-Key:", StringComparison.OrdinalIgnoreCase));
        string key = keyLine.Split(':', 2)[1].Trim();
        string accept = Convert.ToBase64String(SHA1.HashData(Encoding.ASCII.GetBytes(key + WebSocketMagicGuid)));

        string response = "HTTP/1.1 101 Switching Protocols\r\n" +
                          "Upgrade: websocket\r\n" +
                          "Connection: Upgrade\r\n" +
                          "Sec-WebSocket-Accept: " + accept + "\r\n\r\n";
        byte[] bytes = Encoding.ASCII.GetBytes(response);
        await stream.WriteAsync(bytes, ct);
        await stream.FlushAsync(ct);

        return WebSocket.CreateFromStream(stream, isServer: true, subProtocol: null, keepAliveInterval: TimeSpan.Zero);
    }
}
```

- [ ] **Step 2: Scrivi i test del client (RED)**

`tests/LareShell.Tests/Protocol/OrchestratorClientTests.cs`:

```csharp
using System.Text.Json.Nodes;
using LareShell.Config;
using LareShell.Protocol;
using Xunit;

namespace LareShell.Tests.Protocol;

public class OrchestratorClientTests
{
    private static CancellationToken Timeout() => new CancellationTokenSource(TimeSpan.FromSeconds(10)).Token;

    private static OrchestratorClient NewClient(FakeOrchestrator server, string token = "tok") =>
        new(server.Uri, () => token, "sess1", "2.0.0", HostLog.Null);

    [Fact]
    public async Task Connect_manda_hello_con_role_shell_e_riesce_su_server_info()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start(expectedToken: "tok");
        using OrchestratorClient client = NewClient(server);

        Task<JsonObject> accepted = server.AcceptAsync(ct);
        string? reason = await client.ConnectAsync(@"C:\cwd", ct);
        JsonObject hello = await accepted;

        Assert.Null(reason);
        Assert.True(client.IsConnected);
        Assert.Equal("hello", (string?)hello["type"]);
        Assert.Equal("shell", (string?)hello["role"]);
        Assert.Equal("sess1", (string?)hello["session_id"]);
        Assert.Equal(@"C:\cwd", (string?)hello["cwd"]);
        Assert.Equal("2.0.0", (string?)hello["version"]);
    }

    [Fact]
    public async Task Connect_fallisce_con_motivo_se_il_token_e_rifiutato()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start(expectedToken: "buono");
        using OrchestratorClient client = NewClient(server, token: "sbagliato");

        Task<JsonObject> accepted = server.AcceptAsync(ct);
        string? reason = await client.ConnectAsync(@"C:\", ct);
        await accepted;

        Assert.NotNull(reason);
        Assert.False(client.IsConnected);
    }

    [Fact]
    public async Task Connect_fallisce_con_motivo_se_nessuno_ascolta()
    {
        // Porta presa e subito rilasciata: quasi certamente nessuno ci ascolta.
        await using FakeOrchestrator probe = FakeOrchestrator.Start();
        Uri uri = probe.Uri;
        await probe.DisposeAsync();

        using var client = new OrchestratorClient(uri, () => "tok", "s", "2.0.0", HostLog.Null);
        string? reason = await client.ConnectAsync(@"C:\", Timeout());
        Assert.NotNull(reason);
        Assert.False(client.IsConnected);
    }

    [Fact]
    public async Task Connect_fallisce_se_il_token_non_e_disponibile()
    {
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using var client = new OrchestratorClient(server.Uri, () => null, "s", "2.0.0", HostLog.Null);
        string? reason = await client.ConnectAsync(@"C:\", Timeout());
        Assert.Contains("token", reason, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public async Task Incoming_consegna_i_messaggi_parsati_in_ordine()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        await server.SendAsync("{\"type\":\"chunk\",\"id\":\"t\",\"content\":\"a\"}", ct);
        await server.SendAsync("{\"type\":\"exec_in_shell\",\"turn_id\":\"t\",\"exec_id\":\"e\",\"command\":\"dir\",\"capture\":true}", ct);
        await server.SendAsync("{\"type\":\"done\",\"id\":\"t\",\"exit_code\":null}", ct);

        Assert.IsType<Chunk>(await client.Incoming.ReadAsync(ct));
        Assert.IsType<ExecInShell>(await client.Incoming.ReadAsync(ct));
        Assert.IsType<Done>(await client.Incoming.ReadAsync(ct));
    }

    [Fact]
    public async Task Un_messaggio_spezzato_in_due_frame_viene_riassemblato()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        string content = new('x', 50_000);
        await server.SendFragmentedAsync("{\"type\":\"chunk\",\"id\":\"t\",\"content\":\"" + content + "\"}", splitAt: 20_000, ct);

        Chunk c = Assert.IsType<Chunk>(await client.Incoming.ReadAsync(ct));
        Assert.Equal(content, c.Content);
    }

    [Fact]
    public async Task Send_arriva_al_server()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        Assert.True(await client.SendAsync(Wire.Command("id1", "/ping", @"C:\"), ct));
        JsonObject got = await server.ReceiveAsync(ct);
        Assert.Equal("command", (string?)got["type"]);
        Assert.Equal("/ping", (string?)got["input"]);
    }

    [Fact]
    public async Task Send_senza_connessione_ritorna_false_senza_lanciare()
    {
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Assert.False(await client.SendAsync(Wire.Ping(1), Timeout()));
    }

    [Fact]
    public async Task Chiusura_dal_server_accoda_Disconnected_e_IsConnected_diventa_false()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        await server.CloseAsync();

        Disconnected d = Assert.IsType<Disconnected>(await client.Incoming.ReadAsync(ct));
        Assert.False(string.IsNullOrEmpty(d.Reason));
        Assert.False(client.IsConnected);
    }

    [Fact]
    public async Task Tipo_sconosciuto_e_JSON_rotto_non_fermano_il_loop()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        await server.SendAsync("{\"type\":\"cwd\",\"path\":\"C:\\\\\"}", ct);   // messaggio v1 per la ui
        await server.SendAsync("questo non è json", ct);                      // scartato con log
        await server.SendAsync("{\"type\":\"heartbeat\",\"id\":\"t\"}", ct);

        Assert.Equal("cwd", Assert.IsType<Unknown>(await client.Incoming.ReadAsync(ct)).Type);
        Assert.IsType<Heartbeat>(await client.Incoming.ReadAsync(ct));
    }

    [Fact]
    public async Task Riconnessione_dopo_una_caduta_rifa_hello_con_la_stessa_sessione()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);

        Task<JsonObject> first = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await first;
        await server.CloseAsync();
        Assert.IsType<Disconnected>(await client.Incoming.ReadAsync(ct));

        Task<JsonObject> second = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\altro", ct));
        JsonObject hello2 = await second;
        Assert.Equal("sess1", (string?)hello2["session_id"]);
        Assert.Equal(@"C:\altro", (string?)hello2["cwd"]);
        Assert.True(client.IsConnected);
    }
}
```

- [ ] **Step 3: Verifica che fallisca (RED)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~OrchestratorClient"
```

Atteso: `CS0246` su `OrchestratorClient`.

- [ ] **Step 4: Implementa `OrchestratorClient.cs`**

Regola per tutto il file: **ogni `await` porta `.ConfigureAwait(false)`** (omesso sotto per
leggibilità). Il REPL chiama i ponti sincroni `Connect`/`Send` (`.GetAwaiter().GetResult()`): se
una continuazione cercasse di tornare su un SynchronizationContext del thread chiamante mentre
quel thread è bloccato in attesa, sarebbe un deadlock. Con `ConfigureAwait(false)` le continuazioni
restano sul pool.

```csharp
using System.Net.Sockets;
using System.Net.WebSockets;
using System.Text;
using System.Threading.Channels;
using LareShell.Config;

namespace LareShell.Protocol;

/// <summary>
/// Connessione WS persistente verso l'orchestratore (spec §4): handshake <c>hello</c>
/// (role shell, session_id, cwd, version) → <c>server_info</c>; poi un loop di ricezione su un
/// thread del pool che parsa ogni messaggio e lo ACCODA in un <c>Channel</c>. È l'unico ruolo del
/// thread del socket: non tocca mai console né runspace (§4.4 — "il thread WS accoda; il REPL
/// consuma"). Quando la connessione cade, accoda un <c>Disconnected</c> sintetico.
///
/// Nessuna riconnessione automatica in background: chi usa il client (Repl/Launcher) richiama
/// <c>Connect</c> quando serve (ruling 2 del piano). <c>tokenProvider</c> viene invocato a ogni
/// connessione: il file <c>token</c> può non esistere ancora al primo tentativo (l'orchestratore
/// lo crea all'avvio).
/// </summary>
internal sealed class OrchestratorClient : IDisposable
{
    private static readonly TimeSpan SyncTimeout = TimeSpan.FromSeconds(5);

    private readonly Uri _uri;
    private readonly Func<string?> _tokenProvider;
    private readonly string _version;
    private readonly HostLog _log;
    private readonly Channel<ServerMessage> _incoming = Channel.CreateUnbounded<ServerMessage>(
        new UnboundedChannelOptions { SingleReader = true, SingleWriter = true });
    private readonly SemaphoreSlim _sendLock = new(1, 1);

    private ClientWebSocket? _socket;
    private Task? _receiveLoop;

    public OrchestratorClient(Uri uri, Func<string?> tokenProvider, string sessionId, string version, HostLog log)
    {
        _uri = uri;
        _tokenProvider = tokenProvider;
        SessionId = sessionId;
        _version = version;
        _log = log;
    }

    public string SessionId { get; }

    /// <summary>Il lato di lettura del canale: lo consuma SOLO il thread del REPL.</summary>
    public ChannelReader<ServerMessage> Incoming => _incoming.Reader;

    public bool IsConnected => _socket is { State: WebSocketState.Open };

    /// <summary>Connette e fa l'handshake. Ritorna <c>null</c> se connesso, altrimenti un motivo
    /// leggibile (da stampare nel terminale). Non lancia per gli errori di rete attesi.</summary>
    public async Task<string?> ConnectAsync(string cwd, CancellationToken ct)
    {
        string? token = _tokenProvider();
        if (string.IsNullOrEmpty(token))
        {
            return "token non trovato (l'orchestratore lo crea al primo avvio)";
        }

        await ShutdownSocketAsync();

        var socket = new ClientWebSocket();
        try
        {
            await socket.ConnectAsync(_uri, ct);
            await SendRawAsync(socket, Wire.Hello(token, SessionId, cwd, _version), ct);

            string? first = await ReceiveTextAsync(socket, ct);
            if (first is null)
            {
                socket.Dispose();
                return "token rifiutato o connessione chiusa durante l'handshake";
            }

            if (Wire.Parse(first) is not ServerInfo)
            {
                socket.Dispose();
                return "handshake inatteso: " + first;
            }
        }
        catch (Exception ex) when (ex is WebSocketException or SocketException or IOException or HttpRequestException or WireException)
        {
            socket.Dispose();
            return "connessione fallita: " + ex.Message;
        }

        _socket = socket;
        _receiveLoop = Task.Run(() => ReceiveLoopAsync(socket));
        _log.Info("connesso a " + _uri + " (sessione " + SessionId + ")");
        return null;
    }

    /// <summary>Ponte sincrono per il thread del REPL (che non è async: vedi Global Constraints).</summary>
    public string? Connect(string cwd, TimeSpan timeout)
    {
        using var cts = new CancellationTokenSource(timeout);
        try
        {
            return ConnectAsync(cwd, cts.Token).GetAwaiter().GetResult();
        }
        catch (OperationCanceledException)
        {
            return "timeout di connessione (" + timeout.TotalSeconds + " s)";
        }
    }

    /// <summary>Invia un messaggio. <c>false</c> se non connessi o se l'invio fallisce (mai un'eccezione).</summary>
    public async Task<bool> SendAsync(string json, CancellationToken ct)
    {
        ClientWebSocket? socket = _socket;
        if (socket is not { State: WebSocketState.Open })
        {
            return false;
        }

        try
        {
            // Un solo SendAsync alla volta per socket: è un vincolo di ClientWebSocket.
            await _sendLock.WaitAsync(ct);
            try
            {
                await SendRawAsync(socket, json, ct);
            }
            finally
            {
                _sendLock.Release();
            }

            return true;
        }
        catch (Exception ex) when (ex is WebSocketException or IOException or ObjectDisposedException or OperationCanceledException)
        {
            _log.Warn("invio fallito: " + ex.Message);
            return false;
        }
    }

    public bool Send(string json)
    {
        using var cts = new CancellationTokenSource(SyncTimeout);
        return SendAsync(json, cts.Token).GetAwaiter().GetResult();
    }

    public void Dispose()
    {
        ShutdownSocketAsync().GetAwaiter().GetResult();
        _sendLock.Dispose();
    }

    private async Task ReceiveLoopAsync(ClientWebSocket socket)
    {
        string reason;
        try
        {
            while (true)
            {
                string? text = await ReceiveTextAsync(socket, CancellationToken.None);
                if (text is null)
                {
                    reason = "chiusa dall'orchestratore";
                    break;
                }

                ServerMessage message;
                try
                {
                    message = Wire.Parse(text);
                }
                catch (WireException ex)
                {
                    _log.Warn("messaggio scartato: " + ex.Message);
                    continue;
                }

                _incoming.Writer.TryWrite(message);
            }
        }
        catch (Exception ex) when (ex is WebSocketException or IOException or ObjectDisposedException or OperationCanceledException)
        {
            reason = ex.Message;
        }

        _log.Info("connessione chiusa: " + reason);
        _incoming.Writer.TryWrite(new Disconnected(reason));
    }

    /// <summary>Legge UN messaggio di testo completo, riassemblando i frame fino a EndOfMessage
    /// (un messaggio lungo arriva spezzato). <c>null</c> = frame di chiusura.</summary>
    private static async Task<string?> ReceiveTextAsync(WebSocket socket, CancellationToken ct)
    {
        var buffer = new byte[16 * 1024];
        using var ms = new MemoryStream();
        while (true)
        {
            WebSocketReceiveResult r = await socket.ReceiveAsync(buffer, ct);
            if (r.MessageType == WebSocketMessageType.Close)
            {
                return null;
            }

            ms.Write(buffer, 0, r.Count);
            if (r.EndOfMessage)
            {
                return Encoding.UTF8.GetString(ms.ToArray());
            }
        }
    }

    private static Task SendRawAsync(WebSocket socket, string json, CancellationToken ct) =>
        socket.SendAsync(Encoding.UTF8.GetBytes(json), WebSocketMessageType.Text, endOfMessage: true, ct);

    private async Task ShutdownSocketAsync()
    {
        ClientWebSocket? old = _socket;
        _socket = null;
        if (old is null)
        {
            return;
        }

        // Abort (non Close): non vogliamo aspettare un server che magari è morto. Il loop di
        // ricezione esce con un'eccezione e accoda Disconnected; lo attendiamo per non lasciare task in volo.
        try { old.Abort(); } catch { /* già chiuso */ }
        old.Dispose();
        if (_receiveLoop is not null)
        {
            try { await _receiveLoop; } catch { /* le eccezioni del loop sono già gestite dentro */ }
            _receiveLoop = null;
        }
    }
}
```

- [ ] **Step 5: Verifica che passi (GREEN)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~OrchestratorClient"
```

Atteso: 11 test superati, in meno di 15 s totali. Se un test resta appeso, il sospetto n.1 è
`ReceiveAsync` senza `EndOfMessage` o un `Close` non gestito: NON alzare i timeout.

- [ ] **Step 6: Commit**

```powershell
git add shell/lare-shell
git commit -m "feat(lare-shell): OrchestratorClient (hello/server_info, loop di ricezione → Channel) e server WS finto in-process"
```

### Task 3: Classi host (`PSHost`, `PSHostUserInterface`, `PSHostRawUserInterface`) dallo spike + seam `IConsoleModes` + `OutputRecorder`

**Files:**
- Create: `shell/lare-shell/src/LareShell/Host/IConsoleModes.cs`, `ConsoleModes.cs`, `LareHost.cs`, `LareHostUI.cs`,
  `LareRawUI.cs`, `OutputRecorder.cs`
- Test: `shell/lare-shell/tests/LareShell.Tests/Host/OutputRecorderTests.cs`, `Host/LareHostTests.cs`

**Interfaces:**
- Consumes: `HostInfo` (Task 0).
- Produces: `IConsoleModes { IntPtr GetHandle(int std); bool TryGetMode(IntPtr, out uint); bool TrySetMode(IntPtr, uint); }`;
  `ConsoleModes` (statico: P/Invoke + costanti `StdOutputHandle=-11`, `StdInputHandle=-10`, `EnableProcessedOutput=0x0001`,
  `EnableVirtualTerminalProcessing=0x0004`, `TryEnableVirtualTerminalProcessing()`); `Win32ConsoleModes : IConsoleModes`;
  `LareHost()` / `LareHost(IConsoleModes)` con `HostUI` (tipato `LareHostUI`), `ShouldExit`, `ExitCode`, `Name = HostInfo.Name`,
  `Version = 2.0.0`; `LareHostUI.Recorder : OutputRecorder`; `OutputRecorder { Begin(); Append(string); string End(); bool IsRecording; const int MaxChars }`.

Sorgente da copiare: `spikes/lare-shell-host/LareHost.cs`, `LareHostUI.cs`, `LareRawUI.cs`, `ConsoleModes.cs`
(leggili per intero prima di iniziare). Modifiche rispetto allo spike, tutte e sole queste:

1. namespace `LareShell.Host`; nome host `HostInfo.Name`, versione `new Version(HostInfo.Version)`;
2. `LareHost` prende `IConsoleModes` (costruttore senza parametri → `Win32ConsoleModes`); le chiamate statiche
   `ConsoleModes.GetHandle/TryGetMode/TrySetMode` dentro `LareHost` diventano chiamate sull'istanza;
3. rimuovi il riferimento a `StatusBar.Current?.Redraw()` (niente barre VT nella 2.0, D13) e il testo "spike" nei commenti;
4. `LareHostUI` espone `Recorder` e registra ogni testo scritto (tranne `WriteProgress`, rumore); aggiungi `HostUI` tipato su `LareHost`;
5. `PromptForCredential` resta `NotImplementedException` (fuori MVP, come nello spike) — annotalo nel messaggio;
6. `LareRawUI` copiata tal quale (solo namespace).

- [ ] **Step 1: Test del registratore (RED)**

`tests/LareShell.Tests/Host/OutputRecorderTests.cs`:

```csharp
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
```

- [ ] **Step 2: Test dell'host (RED)**

`tests/LareShell.Tests/Host/LareHostTests.cs`:

```csharp
using LareShell.Host;
using Xunit;

namespace LareShell.Tests.Host;

/// <summary>Console mode finte: due "handle" (out/in) con una mode ciascuno.</summary>
internal sealed class FakeConsoleModes : IConsoleModes
{
    public uint OutMode;
    public uint InMode;
    public bool Available = true;
    public int SetCalls;

    private static readonly IntPtr OutHandle = new(11);
    private static readonly IntPtr InHandle = new(10);

    public IntPtr GetHandle(int std) => std == ConsoleModes.StdOutputHandle ? OutHandle : InHandle;

    public bool TryGetMode(IntPtr handle, out uint mode)
    {
        mode = handle == OutHandle ? OutMode : InMode;
        return Available;
    }

    public bool TrySetMode(IntPtr handle, uint mode)
    {
        if (!Available) return false;
        SetCalls++;
        if (handle == OutHandle) OutMode = mode; else InMode = mode;
        return true;
    }
}

public class LareHostTests
{
    [Fact]
    public void Identita_della_host()
    {
        var host = new LareHost(new FakeConsoleModes());
        Assert.Equal("LareShell", host.Name);
        Assert.Equal(new Version(2, 0, 0), host.Version);
        Assert.Same(host.UI, host.HostUI);
    }

    [Fact]
    public void SetShouldExit_segnala_senza_uscire_dal_processo()
    {
        var host = new LareHost(new FakeConsoleModes());
        Assert.False(host.ShouldExit);
        host.SetShouldExit(7);
        Assert.True(host.ShouldExit);
        Assert.Equal(7, host.ExitCode);
    }

    [Fact]
    public void NotifyBegin_ripristina_le_mode_iniziali_e_NotifyEnd_quelle_salvate_con_VT_acceso()
    {
        // spec §4.4: "NotifyBeginApplication/NotifyEndApplication salvano/ripristinano le
        // modalità console (output e input) come ConsoleHost.cs:1227-1270"
        var modes = new FakeConsoleModes { OutMode = 0x0003, InMode = 0x01F7 };   // stato all'avvio
        var host = new LareHost(modes);                                          // le cattura qui

        modes.OutMode = 0x0007;   // PSReadLine/VT hanno cambiato le mode nel frattempo
        modes.InMode = 0x01E0;

        host.NotifyBeginApplication();
        Assert.Equal(0x0003u, modes.OutMode);   // l'app nativa vede la console "come all'avvio"
        Assert.Equal(0x01F7u, modes.InMode);

        host.NotifyEndApplication();
        Assert.Equal(0x01E0u, modes.InMode);                                                             // input: come prima dell'app
        Assert.Equal(0x0007u | ConsoleModes.EnableVirtualTerminalProcessing | ConsoleModes.EnableProcessedOutput, modes.OutMode);
    }

    [Fact]
    public void Notify_annidati_ripristinano_solo_quando_l_ultima_app_finisce()
    {
        var modes = new FakeConsoleModes { OutMode = 0x0003, InMode = 0x01F7 };
        var host = new LareHost(modes);
        modes.OutMode = 0x0007;
        modes.InMode = 0x01E0;

        host.NotifyBeginApplication();   // contatore 1 → mode iniziali
        host.NotifyBeginApplication();   // contatore 2 → invariate
        int callsAfterBegin = modes.SetCalls;
        host.NotifyEndApplication();     // contatore 1 → NON ripristina
        Assert.Equal(callsAfterBegin, modes.SetCalls);
        Assert.Equal(0x01F7u, modes.InMode);
        host.NotifyEndApplication();     // contatore 0 → ripristina
        Assert.Equal(0x01E0u, modes.InMode);
    }

    [Fact]
    public void Senza_console_reale_le_notifiche_sono_no_op()
    {
        var modes = new FakeConsoleModes { Available = false };
        var host = new LareHost(modes);
        host.NotifyBeginApplication();
        host.NotifyEndApplication();
        Assert.Equal(0, modes.SetCalls);
    }

    [Fact]
    public void I_Write_della_UI_finiscono_nel_registratore_quando_attivo()
    {
        var host = new LareHost(new FakeConsoleModes());
        LareHostUI ui = host.HostUI;
        ui.Write("fuori");                  // prima di Begin: non registrato
        ui.Recorder.Begin();
        ui.Write("a");
        ui.WriteLine("b");
        ui.WriteErrorLine("err");
        ui.WriteWarningLine("w");
        ui.Write(ConsoleColor.Red, ConsoleColor.Black, "c");
        string got = ui.Recorder.End();
        Assert.DoesNotContain("fuori", got);
        Assert.Contains("a", got);
        Assert.Contains("b" + Environment.NewLine, got);
        Assert.Contains("err", got);
        Assert.Contains("WARNING: w", got);
        Assert.Contains("c", got);
    }
}
```

- [ ] **Step 3: Verifica che fallisca (RED)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Host"
```

Atteso: `CS0246` su `OutputRecorder`/`LareHost`/`IConsoleModes`.

- [ ] **Step 4: Implementa**

`src/LareShell/Host/IConsoleModes.cs`:

```csharp
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
```

`src/LareShell/Host/ConsoleModes.cs`: copia di `spikes/lare-shell-host/ConsoleModes.cs` con namespace
`LareShell.Host`; nel commento di classe sostituisci il riferimento a `StatusBar` con "Repl (per l'OSC
9001) e Win32ConsoleModes (per LareHost)". Nessun'altra modifica.

`src/LareShell/Host/OutputRecorder.cs`:

```csharp
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
```

`src/LareShell/Host/LareHost.cs`: copia dello spike con queste sostituzioni puntuali —

```csharp
// intestazione
namespace LareShell.Host;

internal sealed class LareHost : PSHost
{
    private readonly Guid _instanceId = Guid.NewGuid();
    private readonly LareHostUI _ui;
    private readonly IConsoleModes _modes;
    // … i campi delle mode restano come nello spike …

    /// <summary>Costruttore di produzione: console mode Win32 vere.</summary>
    public LareHost() : this(Win32ConsoleModes.Instance) { }

    /// <summary>Costruttore con seam: i test passano console mode finte.</summary>
    public LareHost(IConsoleModes modes)
    {
        _modes = modes;
        _ui = new LareHostUI(this);
        if (OperatingSystem.IsWindows())
        {
            try
            {
                IntPtr outHandle = _modes.GetHandle(ConsoleModes.StdOutputHandle);
                IntPtr inHandle = _modes.GetHandle(ConsoleModes.StdInputHandle);
                _consoleModesAvailable =
                    _modes.TryGetMode(outHandle, out _initialOutputMode) &
                    _modes.TryGetMode(inHandle, out _initialInputMode);
            }
            catch
            {
                _consoleModesAvailable = false;
            }
        }
    }

    public override string Name => HostInfo.Name;

    public override Version Version { get; } = new(HostInfo.Version);

    public override PSHostUserInterface UI => _ui;

    /// <summary>La stessa UI, ma tipata: Executor (Task 4) ci accede per il Recorder.</summary>
    public LareHostUI HostUI => _ui;
    // … resto identico allo spike, con `_modes.` al posto di `ConsoleModes.` in Notify*Application,
    //     e SENZA la riga `StatusBar.Current?.Redraw();` in NotifyEndApplication …
}
```

`src/LareShell/Host/LareHostUI.cs`: copia dello spike con —

```csharp
namespace LareShell.Host;

internal sealed class LareHostUI : PSHostUserInterface
{
    private readonly LareHost _host;
    private readonly LareRawUI _rawUi = new();

    /// <summary>Registratore dell'output per capture:true (spec §4.5 "cattura via
    /// PSHostUserInterface.Write*"): ogni Write* scrive sulla console E, se il registratore è
    /// attivo, anche lì. Così cmdlet e (con il cmdlet di passaggio, Task 4) programmi nativi
    /// tornano all'AI esattamente come li ha visti l'utente.</summary>
    public OutputRecorder Recorder { get; } = new();

    public override void Write(string value)
    {
        Recorder.Append(value);
        SafeConsole(() => Console.Write(value));
    }

    public override void Write(ConsoleColor foregroundColor, ConsoleColor backgroundColor, string value)
    {
        Recorder.Append(value);
        WriteColored(foregroundColor, backgroundColor, value);
    }

    public override void WriteLine()
    {
        Recorder.Append(Environment.NewLine);
        SafeConsole(() => Console.WriteLine());
    }

    public override void WriteLine(string value)
    {
        Recorder.Append(value + Environment.NewLine);
        SafeConsole(() => Console.WriteLine(value));
    }

    // WriteErrorLine/WriteDebugLine/WriteVerboseLine/WriteWarningLine restano come nello spike:
    // passano da Write(colore, colore, testo) e quindi vengono registrati una volta sola.

    public override void WriteProgress(long sourceId, ProgressRecord record)
    {
        // Come nello spike, ma tramite WriteColored (NON Write): le barre di progresso non vanno
        // nell'output catturato — sarebbero solo rumore per l'AI.
        // … stesso corpo dello spike, con `WriteColored(ConsoleColor.DarkGray, ConsoleColor.Black, "[progress] " + status + Environment.NewLine);`
    }

    /// <summary>Scrittura colorata sulla console, senza registrazione (ex corpo di Write(colore…)).</summary>
    private static void WriteColored(ConsoleColor foregroundColor, ConsoleColor backgroundColor, string value)
    {
        SafeConsole(() =>
        {
            ConsoleColor prevFg = Console.ForegroundColor;
            ConsoleColor prevBg = Console.BackgroundColor;
            try
            {
                Console.ForegroundColor = foregroundColor;
                Console.BackgroundColor = backgroundColor;
                Console.Write(value);
            }
            finally
            {
                Console.ForegroundColor = prevFg;
                Console.BackgroundColor = prevBg;
            }
        });
    }
    // … ReadLine, ReadLineAsSecureString, Prompt, PromptForChoice, PromptForCredential, SafeConsole: come nello spike …
}
```

- [ ] **Step 5: Verifica che passi (GREEN)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Host"
```

Atteso: 11 test superati (5 recorder + 6 host).

- [ ] **Step 6: Commit**

```powershell
git add shell/lare-shell
git commit -m "feat(lare-shell): classi host PSHost/UI/RawUI dallo spike, seam IConsoleModes, OutputRecorder (cap 200 KB testa+coda)"
```

### Task 4: Runspace ospitata (PSReadLine da pwsh, execution policy da file), profili, `Executor` (capture true/false, exit code, cwd, Stop)

**Files:**
- Create: `shell/lare-shell/src/LareShell/Shell/PwshLocator.cs`, `RunspaceSession.cs`, `ProfileLoader.cs`, `Executor.cs`
- Test: `shell/lare-shell/tests/LareShell.Tests/Shell/PwshLocatorTests.cs`, `RunspaceSessionTests.cs`, `ProfileLoaderTests.cs`,
  `ExecutorTests.cs`, e la definizione della collection `RunspaceCollection.cs`

**Interfaces:**
- Consumes: `LareHost`, `LareHostUI.Recorder`, `ConsoleModes` (Task 3); `HostInfo`, `HostLog` (Task 0).
- Produces: `PwshLocator.FindInstallDir(string? pathEnv, Func<string,bool> fileExists) → string?` (pura) e
  `PwshLocator.FindInstallDir() → string?` (PATH → registro → Program Files);
  `RunspaceSession.Open(LareHost host, HostLog log) → RunspaceSession` (`IDisposable`) con `Host`, `Runspace`,
  `PsReadLineAvailable`, `PwshDir`, `CurrentDirectory`, `EvaluatePrompt()`, `ReadLine(bool usePsReadLine) → string?`,
  `static bool FunctionExists(Runspace, string)`, `static void PrependModulePath(string modulesDir)`;
  `ProfileLoader.Compute(documentsDir, pwshDir, appDir) → ProfilePaths`, `LoadOrder(ProfilePaths)`,
  `SetDollarProfile(Runspace, ProfilePaths)`, `Load(Runspace, PSHost, ProfilePaths, Func<string,bool> exists) → IReadOnlyList<string>`;
  `interface IExecutor { ExecOutcome Run(string command, bool capture); void StopCurrent(); }`,
  `record ExecOutcome(int ExitCode, string Output, string Cwd, bool Stopped)`, `Executor(RunspaceSession)` con in più
  `bool RunInteractive(string line)` (comandi digitati: nessuna cattura, nessuna lettura di `$?`).

- [ ] **Step 1: Test di `PwshLocator` (RED)**

`tests/LareShell.Tests/Shell/PwshLocatorTests.cs`:

```csharp
using LareShell.Shell;
using Xunit;

namespace LareShell.Tests.Shell;

public class PwshLocatorTests
{
    [Fact]
    public void Trova_la_prima_cartella_del_PATH_che_contiene_pwsh_exe()
    {
        string path = @"C:\Windows;C:\Program Files\PowerShell\7;C:\altro\pwsh";
        string? dir = PwshLocator.FindInstallDir(path, f => f.Equals(@"C:\Program Files\PowerShell\7\pwsh.exe", StringComparison.OrdinalIgnoreCase));
        Assert.Equal(@"C:\Program Files\PowerShell\7", dir);
    }

    [Fact]
    public void Ignora_voci_vuote_e_virgolette()
    {
        string path = @";""C:\pw"";";
        string? dir = PwshLocator.FindInstallDir(path, f => f.Equals(@"C:\pw\pwsh.exe", StringComparison.OrdinalIgnoreCase));
        Assert.Equal(@"C:\pw", dir);
    }

    [Fact]
    public void Null_se_nessuna_cartella_ha_pwsh()
    {
        Assert.Null(PwshLocator.FindInstallDir(@"C:\a;C:\b", _ => false));
        Assert.Null(PwshLocator.FindInstallDir(null, _ => true));
    }

    [Fact]
    public void Su_questa_macchina_pwsh_7_viene_trovato()
    {
        // Prerequisito dichiarato (spec §7): pwsh 7.6+ installato. Se fallisce qui, fallisce tutto il resto.
        string? dir = PwshLocator.FindInstallDir();
        Assert.NotNull(dir);
        Assert.True(File.Exists(Path.Combine(dir!, "pwsh.exe")));
        Assert.True(Directory.Exists(Path.Combine(dir!, "Modules", "PSReadLine")), "manca Modules\\PSReadLine in " + dir);
    }
}
```

- [ ] **Step 2: Implementa `PwshLocator`**

```csharp
using Microsoft.Win32;

namespace LareShell.Shell;

/// <summary>
/// Trova la cartella di installazione di pwsh (es. C:\Program Files\PowerShell\7). Serve per
/// PSReadLine: il NuGet Microsoft.PowerShell.SDK NON lo include (spec §7) e su questa macchina
/// <c>&lt;pwsh&gt;\Modules</c> non è nel PSModulePath di macchina/utente — è pwsh.exe stesso ad
/// aggiungere il proprio <c>$PSHOME\Modules</c> al PSModulePath del suo processo. Una host che
/// gira nuda in Windows Terminal non lo eredita: lo aggiungiamo noi (RunspaceSession.Open).
/// Ordine: PATH → registro (HKLM\SOFTWARE\Microsoft\PowerShellCore\InstalledVersions\*\InstallLocation)
/// → C:\Program Files\PowerShell\7. Leggere PATH/registro NON è configurazione Lare (D6).
/// </summary>
internal static class PwshLocator
{
    /// <summary>Parte pura, testabile: scandisce un PATH dato con un predicato di esistenza.</summary>
    public static string? FindInstallDir(string? pathEnv, Func<string, bool> fileExists)
    {
        if (string.IsNullOrEmpty(pathEnv))
        {
            return null;
        }

        foreach (string raw in pathEnv.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            string dir = raw.Trim('"');
            if (dir.Length == 0)
            {
                continue;
            }

            if (fileExists(Path.Combine(dir, "pwsh.exe")))
            {
                return dir;
            }
        }

        return null;
    }

    public static string? FindInstallDir()
    {
        string? fromPath = FindInstallDir(Environment.GetEnvironmentVariable("PATH"), File.Exists);
        if (fromPath is not null)
        {
            return fromPath;
        }

        if (OperatingSystem.IsWindows())
        {
            try
            {
                using RegistryKey? versions = Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\PowerShellCore\InstalledVersions");
                foreach (string sub in versions?.GetSubKeyNames() ?? Array.Empty<string>())
                {
                    using RegistryKey? key = versions!.OpenSubKey(sub);
                    if (key?.GetValue("InstallLocation") is string loc && File.Exists(Path.Combine(loc, "pwsh.exe")))
                    {
                        return loc.TrimEnd('\\');
                    }
                }
            }
            catch (Exception ex) when (ex is System.Security.SecurityException or IOException or UnauthorizedAccessException)
            {
                // Registro non leggibile: si passa al fallback.
            }
        }

        string fallback = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "PowerShell", "7");
        return File.Exists(Path.Combine(fallback, "pwsh.exe")) ? fallback : null;
    }
}
```

Esegui `dotnet test … --filter "FullyQualifiedName~PwshLocator"`: 4 test verdi.

- [ ] **Step 3: Test della runspace e dei profili (RED)**

`tests/LareShell.Tests/Shell/RunspaceCollection.cs`:

```csharp
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
```

`tests/LareShell.Tests/Shell/RunspaceSessionTests.cs`:

```csharp
using System.Management.Automation;
using LareShell.Config;
using LareShell.Host;
using LareShell.Shell;
using LareShell.Tests.Host;
using Xunit;

namespace LareShell.Tests.Shell;

[Collection("runspace")]
public class RunspaceSessionTests
{
    [Fact]
    public void Open_carica_PSReadLine_dai_moduli_di_pwsh()
    {
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        Assert.True(s.PsReadLineAvailable, "PSConsoleHostReadLine assente: PSReadLine non caricato");
        Assert.NotNull(s.PwshDir);

        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("(Get-Module PSReadLine).Path");
        string path = ps.Invoke().Single().BaseObject.ToString()!;
        Assert.StartsWith(s.PwshDir!, path, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void La_execution_policy_LocalMachine_viene_dal_powershell_config_json_accanto_all_exe()
    {
        // spec §4.4/§13: "risolverla come ConsoleHost, non forzarla". Il file è copiato
        // nell'output (CopyToOutputDirectory) e PowerShell lo legge da $PSHOME.
        Assert.True(File.Exists(Path.Combine(AppContext.BaseDirectory, "powershell.config.json")));
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("(Get-ExecutionPolicy -Scope LocalMachine).ToString()");
        Assert.Equal("RemoteSigned", ps.Invoke().Single().BaseObject.ToString());
    }

    [Fact]
    public void CurrentDirectory_segue_Set_Location_del_runspace_non_la_cwd_del_processo()
    {
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        string temp = Path.GetTempPath().TrimEnd('\\');
        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("Set-Location '" + temp + "'").Invoke();
        Assert.Equal(temp, s.CurrentDirectory.TrimEnd('\\'), ignoreCase: true);
    }

    [Fact]
    public void EvaluatePrompt_usa_la_funzione_prompt_dell_utente_con_fallback()
    {
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        Assert.False(string.IsNullOrWhiteSpace(s.EvaluatePrompt()));   // default: "PS C:\…> "
        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("function global:prompt { 'lare> ' }").Invoke();
        Assert.Equal("lare> ", s.EvaluatePrompt());
    }

    [Fact]
    public void PrependModulePath_antepone_una_sola_volta()
    {
        string original = Environment.GetEnvironmentVariable("PSModulePath") ?? string.Empty;
        try
        {
            RunspaceSession.PrependModulePath(@"C:\finta\Modules");
            RunspaceSession.PrependModulePath(@"C:\finta\Modules\");
            string now = Environment.GetEnvironmentVariable("PSModulePath")!;
            Assert.StartsWith(@"C:\finta\Modules;", now);
            Assert.Equal(1, now.Split(';').Count(p => p.TrimEnd('\\').Equals(@"C:\finta\Modules", StringComparison.OrdinalIgnoreCase)));
        }
        finally
        {
            Environment.SetEnvironmentVariable("PSModulePath", original);
        }
    }
}
```

`tests/LareShell.Tests/Shell/ProfileLoaderTests.cs`:

```csharp
using System.Management.Automation;
using LareShell.Config;
using LareShell.Host;
using LareShell.Shell;
using LareShell.Tests.Host;
using Xunit;

namespace LareShell.Tests.Shell;

public class ProfilePathsTests
{
    [Fact]
    public void Compute_da_i_cinque_percorsi_con_i_nomi_di_pwsh_e_della_host()
    {
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\Users\u\Documents", @"C:\Program Files\PowerShell\7", @"C:\Lare\shell");
        Assert.Equal(@"C:\Program Files\PowerShell\7\profile.ps1", p.AllUsersAllHosts);
        Assert.Equal(@"C:\Program Files\PowerShell\7\LareShell_profile.ps1", p.AllUsersCurrentHost);
        Assert.Equal(@"C:\Users\u\Documents\PowerShell\profile.ps1", p.CurrentUserAllHosts);
        Assert.Equal(@"C:\Users\u\Documents\PowerShell\LareShell_profile.ps1", p.CurrentUserCurrentHost);
        Assert.Equal(@"C:\Users\u\Documents\PowerShell\Microsoft.PowerShell_profile.ps1", p.PwshCurrentHost);
    }

    [Fact]
    public void Senza_pwsh_i_profili_AllUsers_puntano_alla_cartella_dell_app()
    {
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\Users\u\Documents", null, @"C:\Lare\shell");
        Assert.Equal(@"C:\Lare\shell\profile.ps1", p.AllUsersAllHosts);
    }

    [Fact]
    public void LoadOrder_e_profile_poi_quello_di_pwsh_poi_quello_della_host()
    {
        // spec §4.4: profile.ps1, Microsoft.PowerShell_profile.ps1 (raccomandazione: alias e
        // oh-my-posh dell'utente appaiono in Lare come in pwsh), LareShell_profile.ps1.
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\d", @"C:\p", @"C:\a");
        Assert.Equal(new[] { p.CurrentUserAllHosts, p.PwshCurrentHost, p.CurrentUserCurrentHost }, ProfileLoader.LoadOrder(p));
    }
}

[Collection("runspace")]
public class ProfileLoaderRunspaceTests
{
    [Fact]
    public void Load_dot_sourcia_i_profili_esistenti_in_ordine_e_continua_dopo_un_errore()
    {
        string docs = Path.Combine(Path.GetTempPath(), "lare-prof-" + Guid.NewGuid().ToString("N"));
        string dir = Path.Combine(docs, "PowerShell");
        Directory.CreateDirectory(dir);
        try
        {
            File.WriteAllText(Path.Combine(dir, "profile.ps1"), "function global:lare_t1 { 1 }\r\n$global:lare_order = 'a'\r\n");
            File.WriteAllText(Path.Combine(dir, "Microsoft.PowerShell_profile.ps1"), "$global:lare_order += 'b'\r\nGet-Item 'C:\\__lare_non_esiste__'\r\nfunction global:lare_t2 { 2 }\r\n");
            File.WriteAllText(Path.Combine(dir, "LareShell_profile.ps1"), "$global:lare_order += 'c'\r\n");

            var host = new LareHost(new FakeConsoleModes());
            using RunspaceSession s = RunspaceSession.Open(host, HostLog.Null);
            ProfileLoader.ProfilePaths p = ProfileLoader.Compute(docs, s.PwshDir, AppContext.BaseDirectory);

            host.HostUI.Recorder.Begin();
            IReadOnlyList<string> loaded = ProfileLoader.Load(s.Runspace, host, p, File.Exists);
            string printed = host.HostUI.Recorder.End();

            Assert.Equal(3, loaded.Count);
            Assert.True(RunspaceSession.FunctionExists(s.Runspace, "lare_t1"));
            Assert.True(RunspaceSession.FunctionExists(s.Runspace, "lare_t2"), "l'errore non terminante non deve fermare il profilo");
            Assert.Equal("abc", s.Runspace.SessionStateProxy.GetVariable("lare_order"));
            Assert.Contains("__lare_non_esiste__", printed);   // l'errore è stato mostrato, non inghiottito
        }
        finally
        {
            Directory.Delete(docs, recursive: true);
        }
    }

    [Fact]
    public void Load_salta_i_profili_assenti_senza_errori()
    {
        var host = new LareHost(new FakeConsoleModes());
        using RunspaceSession s = RunspaceSession.Open(host, HostLog.Null);
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\__nessuna_cartella__", null, AppContext.BaseDirectory);
        Assert.Empty(ProfileLoader.Load(s.Runspace, host, p, File.Exists));
    }

    [Fact]
    public void SetDollarProfile_espone_la_stringa_e_le_quattro_NoteProperty_di_pwsh()
    {
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\d", @"C:\p", @"C:\a");
        ProfileLoader.SetDollarProfile(s.Runspace, p);

        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("\"$PROFILE|$($PROFILE.AllUsersAllHosts)|$($PROFILE.AllUsersCurrentHost)|$($PROFILE.CurrentUserAllHosts)|$($PROFILE.CurrentUserCurrentHost)\"");
        string got = ps.Invoke().Single().BaseObject.ToString()!;
        Assert.Equal(@"C:\d\PowerShell\LareShell_profile.ps1|C:\p\profile.ps1|C:\p\LareShell_profile.ps1|C:\d\PowerShell\profile.ps1|C:\d\PowerShell\LareShell_profile.ps1", got);
    }
}
```

- [ ] **Step 4: Verifica che fallisca (RED)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~RunspaceSession|FullyQualifiedName~Profile"
```

Atteso: `CS0246` su `RunspaceSession`/`ProfileLoader`.

- [ ] **Step 5: Implementa `RunspaceSession` e `ProfileLoader`**

`src/LareShell/Shell/RunspaceSession.cs`:

```csharp
using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Management.Automation.Runspaces;
using LareShell.Config;
using LareShell.Host;

namespace LareShell.Shell;

/// <summary>
/// La runspace ospitata (il "motore" PowerShell dentro il nostro processo) con tutto ciò che il
/// REPL le chiede: PSReadLine per leggere le righe (come pwsh.exe: invoca la funzione
/// PSConsoleHostReadLine), la funzione <c>prompt</c> dell'utente, la cwd del runspace.
/// Possiede la runspace: <c>Dispose</c> la chiude. Va aperta SUL thread del REPL
/// (Runspace.DefaultRunspace è per thread).
/// </summary>
internal sealed class RunspaceSession : IDisposable
{
    private RunspaceSession(LareHost host, Runspace runspace, bool psReadLineAvailable, string? pwshDir)
    {
        Host = host;
        Runspace = runspace;
        PsReadLineAvailable = psReadLineAvailable;
        PwshDir = pwshDir;
    }

    public LareHost Host { get; }

    public Runspace Runspace { get; }

    public bool PsReadLineAvailable { get; }

    /// <summary>Cartella di pwsh trovata da PwshLocator (null = non trovata: niente PSReadLine).</summary>
    public string? PwshDir { get; }

    /// <summary>cwd DEL RUNSPACE (spec §4.6): è quella che vede Set-Location, non
    /// Directory.GetCurrentDirectory() del processo (le due possono divergere).</summary>
    public string CurrentDirectory => Runspace.SessionStateProxy.Path.CurrentLocation.ProviderPath;

    public static RunspaceSession Open(LareHost host, HostLog log)
    {
        // 1. PSReadLine vive nei moduli di pwsh, non nel NuGet: anteponiamo la sua cartella al
        //    PSModulePath del PROCESSO prima di creare la runspace (il motore calcola il proprio
        //    PSModulePath all'apertura, partendo da quello del processo).
        string? pwshDir = PwshLocator.FindInstallDir();
        if (pwshDir is not null)
        {
            PrependModulePath(Path.Combine(pwshDir, "Modules"));
        }
        else
        {
            log.Warn("pwsh non trovato (PATH/registro/Program Files): PSReadLine non disponibile");
        }

        // 2. Stato iniziale come ConsoleHost: moduli di default + PSReadLine importato all'apertura.
        //    NESSUN iss.ExecutionPolicy: la policy LocalMachine viene da powershell.config.json
        //    accanto all'exe (Global Constraints, ADR-019).
        InitialSessionState iss = InitialSessionState.CreateDefault();
        iss.ImportPSModule(new[] { "PSReadLine" });

        Runspace runspace = RunspaceFactory.CreateRunspace(host, iss);
        runspace.Open();
        // Molte API "ambient" del motore assumono una runspace di default per il thread corrente.
        Runspace.DefaultRunspace = runspace;

        bool psReadLine = FunctionExists(runspace, "PSConsoleHostReadLine");
        log.Info("runspace aperta; pwsh=" + (pwshDir ?? "?") + " PSReadLine=" + psReadLine);
        return new RunspaceSession(host, runspace, psReadLine, pwshDir);
    }

    /// <summary>Antepone <paramref name="modulesDir"/> al PSModulePath del processo, una sola volta.</summary>
    internal static void PrependModulePath(string modulesDir)
    {
        string wanted = modulesDir.TrimEnd('\\');
        string current = Environment.GetEnvironmentVariable("PSModulePath") ?? string.Empty;
        bool present = current.Split(';', StringSplitOptions.RemoveEmptyEntries)
            .Any(p => p.TrimEnd('\\').Equals(wanted, StringComparison.OrdinalIgnoreCase));
        if (!present)
        {
            Environment.SetEnvironmentVariable("PSModulePath", current.Length == 0 ? wanted : wanted + ";" + current);
        }
    }

    public static bool FunctionExists(Runspace runspace, string name)
    {
        try
        {
            using var ps = PowerShell.Create();
            ps.Runspace = runspace;
            ps.AddCommand("Get-Command").AddParameter("Name", name).AddParameter("ErrorAction", "SilentlyContinue");
            return ps.Invoke().Count > 0;
        }
        catch
        {
            return false;
        }
    }

    /// <summary>Valuta la funzione <c>prompt</c> (default del motore o ridefinita dal profilo),
    /// come ConsoleHost.EvaluatePrompt; fallback "PS &lt;cwd&gt;&gt; ".</summary>
    public string EvaluatePrompt()
    {
        try
        {
            using var ps = PowerShell.Create();
            ps.Runspace = Runspace;
            Collection<PSObject> result = ps.AddCommand("prompt").Invoke();
            if (result.Count > 0 && result[0].BaseObject is string text && text.Length > 0)
            {
                return text;
            }
        }
        catch
        {
            // prompt rotto dal profilo: fallback sotto.
        }

        return "PS " + CurrentDirectory + "> ";
    }

    /// <summary>Legge una riga: PSReadLine (PSConsoleHostReadLine) se disponibile e richiesto,
    /// altrimenti Console.ReadLine. <c>null</c> = EOF. Lezione dello spike: con stdin rediretto
    /// PSConsoleHostReadLine non dà mai EOF (loop infinito) → il chiamante passa
    /// <paramref name="usePsReadLine"/> = false in quel caso.</summary>
    public string? ReadLine(bool usePsReadLine)
    {
        if (usePsReadLine && PsReadLineAvailable)
        {
            try
            {
                using var ps = PowerShell.Create();
                ps.Runspace = Runspace;
                Collection<PSObject> result = ps.AddCommand("PSConsoleHostReadLine").Invoke();
                // 0 risultati = Ctrl+C durante l'editing: riga vuota, non EOF.
                return result.Count == 1 ? result[0].BaseObject as string ?? string.Empty : string.Empty;
            }
            catch (Exception ex)
            {
                Host.UI.WriteWarningLine("PSConsoleHostReadLine ha fallito (" + ex.GetType().Name + "): fallback a Console.ReadLine per questa riga");
            }
        }

        return Console.ReadLine();
    }

    public void Dispose()
    {
        Runspace.DefaultRunspace = null;
        Runspace.Dispose();
    }
}
```

`src/LareShell/Shell/ProfileLoader.cs`:

```csharp
using System.Management.Automation;
using System.Management.Automation.Host;
using System.Management.Automation.Runspaces;

namespace LareShell.Shell;

/// <summary>
/// I profili (spec §4.4). In una host custom $PROFILE NON viene popolato dal motore (lo fa
/// ConsoleHost stesso, HostUtilities.GetDollarProfile è internal): calcoliamo i percorsi con la
/// stessa convenzione di pwsh (Documents\PowerShell, nome file da $Host.Name) e li esponiamo come
/// pwsh (stringa = CurrentUserCurrentHost + 4 NoteProperty). Carichiamo, in ordine:
/// profile.ps1 · Microsoft.PowerShell_profile.ps1 (quello di pwsh: alias, oh-my-posh, moduli
/// dell'utente appaiono in Lare come in pwsh) · LareShell_profile.ps1. I profili AllUsers (nel
/// $PSHOME di pwsh) NON vengono caricati — debito dichiarato in HANDOFF.
/// </summary>
internal static class ProfileLoader
{
    internal sealed record ProfilePaths(
        string AllUsersAllHosts,
        string AllUsersCurrentHost,
        string CurrentUserAllHosts,
        string CurrentUserCurrentHost,
        string PwshCurrentHost);

    public static ProfilePaths Compute(string documentsDir, string? pwshDir, string appDir)
    {
        string userDir = Path.Combine(documentsDir, "PowerShell");
        string allUsersDir = pwshDir ?? appDir;
        return new ProfilePaths(
            AllUsersAllHosts: Path.Combine(allUsersDir, "profile.ps1"),
            AllUsersCurrentHost: Path.Combine(allUsersDir, HostInfo.Name + "_profile.ps1"),
            CurrentUserAllHosts: Path.Combine(userDir, "profile.ps1"),
            CurrentUserCurrentHost: Path.Combine(userDir, HostInfo.Name + "_profile.ps1"),
            PwshCurrentHost: Path.Combine(userDir, "Microsoft.PowerShell_profile.ps1"));
    }

    public static IReadOnlyList<string> LoadOrder(ProfilePaths p) =>
        new[] { p.CurrentUserAllHosts, p.PwshCurrentHost, p.CurrentUserCurrentHost };

    /// <summary>$PROFILE come in pwsh: la stringa è CurrentUserCurrentHost, con 4 NoteProperty.</summary>
    public static void SetDollarProfile(Runspace runspace, ProfilePaths p)
    {
        PSObject profile = PSObject.AsPSObject(p.CurrentUserCurrentHost);
        profile.Properties.Add(new PSNoteProperty("AllUsersAllHosts", p.AllUsersAllHosts));
        profile.Properties.Add(new PSNoteProperty("AllUsersCurrentHost", p.AllUsersCurrentHost));
        profile.Properties.Add(new PSNoteProperty("CurrentUserAllHosts", p.CurrentUserAllHosts));
        profile.Properties.Add(new PSNoteProperty("CurrentUserCurrentHost", p.CurrentUserCurrentHost));
        runspace.SessionStateProxy.SetVariable("PROFILE", profile);
    }

    /// <summary>Dot-source dei profili esistenti nell'ordine di <see cref="LoadOrder"/>. Un errore
    /// in un profilo viene mostrato (Out-Default, come ConsoleHost.RunProfile) e si prosegue col
    /// successivo. Ritorna i percorsi effettivamente caricati.</summary>
    public static IReadOnlyList<string> Load(Runspace runspace, PSHost host, ProfilePaths p, Func<string, bool> exists)
    {
        var loaded = new List<string>();
        foreach (string path in LoadOrder(p))
        {
            if (!exists(path))
            {
                continue;
            }

            try
            {
                using var ps = PowerShell.Create();
                ps.Runspace = runspace;
                // ". '<path>'" = dot-sourcing: esegue nello scope corrente (le funzioni definite
                // restano); l'apice è raddoppiato per i percorsi con apostrofi. Error→Output +
                // Out-Default: gli errori non terminanti si vedono in rosso e non fermano il profilo.
                ps.AddScript(". '" + path.Replace("'", "''") + "'", useLocalScope: false);
                ps.Commands.Commands[0].MergeMyResults(PipelineResultTypes.Error, PipelineResultTypes.Output);
                ps.AddCommand("Out-Default");
                ps.Invoke();
                loaded.Add(path);
            }
            catch (RuntimeException ex)
            {
                host.UI.WriteErrorLine("Errore nel profilo " + path + ": " + ex.Message);
            }
        }

        return loaded;
    }
}
```

Esegui il filtro dello Step 4: 11 test verdi (5 runspace + 3 percorsi + 3 profili). Se
`La_execution_policy_LocalMachine…` dà `Undefined`/`Restricted` con il file presente nel bin: stampa
`$PSHOME` dalla runspace nel report e fermati con **BLOCKED** — è l'assunzione da verificare (spec §13).

- [ ] **Step 6: Test dell'`Executor` (RED)**

`tests/LareShell.Tests/Shell/ExecutorTests.cs`:

```csharp
using LareShell.Config;
using LareShell.Host;
using LareShell.Shell;
using LareShell.Tests.Host;
using Xunit;

namespace LareShell.Tests.Shell;

[Collection("runspace")]
public class ExecutorTests : IDisposable
{
    private readonly LareHost _host = new(new FakeConsoleModes());
    private readonly RunspaceSession _session;
    private readonly Executor _executor;

    public ExecutorTests()
    {
        _session = RunspaceSession.Open(_host, HostLog.Null);
        _executor = new Executor(_session);
    }

    public void Dispose() => _session.Dispose();

    [Fact]
    public void Cmdlet_con_capture_true_torna_output_formattato_exit_0_e_cwd()
    {
        ExecOutcome o = _executor.Run("Get-Date -Format yyyy", capture: true);
        Assert.Equal(0, o.ExitCode);
        Assert.Contains(DateTime.Now.Year.ToString(), o.Output);
        Assert.Equal(_session.CurrentDirectory, o.Cwd);
        Assert.False(o.Stopped);
    }

    [Fact]
    public void Programma_nativo_con_capture_true_viene_catturato_con_il_suo_exit_code()
    {
        // spec §4.5: con un cmdlet in mezzo (ForEach-Object) il nativo NON eredita la console:
        // il suo stdout passa dalla pipe → Out-Default → LareHostUI.Write → Recorder.
        ExecOutcome o = _executor.Run("cmd /c \"echo ciao-nativo & exit 3\"", capture: true);
        Assert.Contains("ciao-nativo", o.Output);
        Assert.Equal(3, o.ExitCode);
    }

    [Fact]
    public void Errore_non_terminante_da_exit_1_e_il_testo_dell_errore_in_output()
    {
        // spec §9: "Pipeline in errore in ExecInShell → testo dell'errore nell'output, exit_code 1"
        ExecOutcome o = _executor.Run("Get-Item 'C:\\__lare_non_esiste__'", capture: true);
        Assert.Equal(1, o.ExitCode);
        Assert.Contains("__lare_non_esiste__", o.Output);
    }

    [Fact]
    public void Errore_di_sintassi_da_exit_1_con_il_messaggio_in_output()
    {
        ExecOutcome o = _executor.Run("if (", capture: true);
        Assert.Equal(1, o.ExitCode);
        Assert.False(string.IsNullOrWhiteSpace(o.Output));
    }

    [Fact]
    public void Dopo_un_comando_fallito_uno_riuscito_torna_a_exit_0()
    {
        _executor.Run("cmd /c exit 5", capture: true);
        ExecOutcome o = _executor.Run("Get-Date", capture: true);
        Assert.Equal(0, o.ExitCode);
    }

    [Fact]
    public void Set_Location_persiste_e_la_cwd_torna_aggiornata()
    {
        // spec §4.6 (D17): "cd persiste" fra un ExecInShell e il successivo — stesso runspace.
        string temp = Path.GetTempPath().TrimEnd('\\');
        ExecOutcome o = _executor.Run("Set-Location '" + temp + "'", capture: true);
        Assert.Equal(temp, o.Cwd.TrimEnd('\\'), ignoreCase: true);
        Assert.Equal(temp, _executor.Run("Get-Location", capture: true).Cwd.TrimEnd('\\'), ignoreCase: true);
    }

    [Fact]
    public void Capture_false_da_output_vuoto_ma_exit_code_e_cwd()
    {
        ExecOutcome o = _executor.Run("cmd /c exit 4", capture: false);
        Assert.Equal(string.Empty, o.Output);
        Assert.Equal(4, o.ExitCode);
        Assert.Equal(_session.CurrentDirectory, o.Cwd);
    }

    [Fact]
    public void StopCurrent_ferma_la_pipeline_e_segna_Stopped()
    {
        // Ctrl+C durante un ExecInShell (spec §4.4): la pipeline viene fermata, niente ExecResult parziale.
        var stopper = new Thread(() =>
        {
            Thread.Sleep(400);
            _executor.StopCurrent();
        });
        stopper.Start();
        ExecOutcome o = _executor.Run("Start-Sleep -Seconds 20", capture: true);
        stopper.Join();
        Assert.True(o.Stopped);
    }

    [Fact]
    public void RunInteractive_esegue_senza_accendere_il_registratore()
    {
        // Comando digitato dall'utente: niente cattura (l'output va solo a schermo) e niente
        // lettura di $? (ruling 3: il prompt dell'utente deve vedere il $? vero).
        bool stopped = _executor.RunInteractive("$global:lare_interactive = 41 + 1");
        Assert.False(stopped);
        Assert.False(_host.HostUI.Recorder.IsRecording);
        Assert.Equal(42, _session.Runspace.SessionStateProxy.GetVariable("lare_interactive"));
    }

    [Fact]
    public void RunInteractive_mostra_gli_errori_e_non_lancia()
    {
        Assert.False(_executor.RunInteractive("if ("));                       // sintassi
        Assert.False(_executor.RunInteractive("Get-Item 'C:\\__lare_no__'")); // non terminante
    }
}
```

- [ ] **Step 7: Verifica che fallisca (RED)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Executor"
```

Atteso: `CS0246` su `Executor`/`ExecOutcome`.

- [ ] **Step 8: Implementa `Executor`**

`src/LareShell/Shell/Executor.cs`:

```csharp
using System.Management.Automation;
using System.Management.Automation.Runspaces;

namespace LareShell.Shell;

/// <summary>Esito di un <c>ExecInShell</c> (spec §4.5): <c>Output</c> vuoto con capture:false;
/// <c>Stopped</c> = fermato da Ctrl+C (niente ExecResult: il turno va cancellato, §4.4).</summary>
internal sealed record ExecOutcome(int ExitCode, string Output, string Cwd, bool Stopped);

/// <summary>Confine d'astrazione: SlashTurn (Task 5) dipende da questo, i test gli passano un falso.</summary>
internal interface IExecutor
{
    ExecOutcome Run(string command, bool capture);

    void StopCurrent();
}

/// <summary>
/// Esegue comandi nel runspace dell'utente. Due modalità (spec §4.5, lezione dello spike):
///  - capture:true  → <c>script | ForEach-Object { $_ } | Out-Default</c>: il cmdlet in mezzo fa
///    sì che un programma nativo NON sia l'ultimo della pipeline, quindi il suo stdout passa
///    dalla pipe → Out-Default → LareHostUI.Write → Recorder; l'output torna all'AI.
///  - capture:false → <c>script | Out-Default</c> puro: il nativo eredita la console vera
///    (editor, REPL, wizard funzionano); output vuoto, restano exit_code e cwd.
/// Errori: Error→Output (MergeMyResults, come ConsoleHost) così gli ErrorRecord sono formattati
/// in rosso e registrati. Va chiamato SOLO dal thread del REPL (una runspace = una pipeline alla volta).
/// </summary>
internal sealed class Executor : IExecutor
{
    /// <summary>Exit code convenzionale per "interrotto da Ctrl+C" (128 + SIGINT), usato solo nei log.</summary>
    private const int StoppedExitCode = 130;

    private readonly RunspaceSession _session;
    private volatile PowerShell? _current;

    public Executor(RunspaceSession session) => _session = session;

    /// <summary>Segnaposto globale in cui lo script cattura il proprio $? (vedi Run).</summary>
    private const string OkVariable = "__lare_ok";

    public ExecOutcome Run(string command, bool capture)
    {
        if (capture)
        {
            _session.Host.HostUI.Recorder.Begin();
        }

        // Prima del comando: $LASTEXITCODE azzerato, così un valore ≠ 0 letto DOPO appartiene a
        // QUESTO comando e non a un nativo fallito in un turno precedente; il segnaposto di $?
        // rimosso, così se il comando non arriva in fondo (errore terminante) la variabile manca.
        SessionStateProxy state = _session.Runspace.SessionStateProxy;
        state.SetVariable("LASTEXITCODE", null);
        state.PSVariable.Remove(OkVariable);

        // $? va letto NELLO stesso script del comando, come ultima istruzione: vale il risultato
        // dell'istruzione precedente, cioè del comando dell'utente (anche per un nativo con exit ≠ 0).
        // Letto da fuori, in una pipeline separata, rifletterebbe la pipeline ESTERNA
        // (… | ForEach-Object | Out-Default), che riesce sempre. L'assegnazione non emette output.
        (bool stopped, bool caughtError) = Invoke(command + "\n$global:" + OkVariable + " = $?", capture);

        string output = capture ? _session.Host.HostUI.Recorder.End() : string.Empty;
        int exitCode = stopped ? StoppedExitCode : caughtError ? 1 : ReadExitCode(state);
        return new ExecOutcome(exitCode, output, _session.CurrentDirectory, stopped);
    }

    /// <summary>Comando digitato dall'utente al prompt: pipeline pura, nessuna cattura, nessuna
    /// istruzione aggiunta, $LASTEXITCODE/$? intatti (che resterebbero alterati per la funzione
    /// prompt — ruling 3). Ritorna true se fermato da Ctrl+C.</summary>
    public bool RunInteractive(string line) => Invoke(line, capture: false).Stopped;

    public void StopCurrent() => _current?.Stop();

    private (bool Stopped, bool CaughtError) Invoke(string script, bool capture)
    {
        using var ps = PowerShell.Create();
        ps.Runspace = _session.Runspace;
        _current = ps;
        try
        {
            ps.AddScript(script, useLocalScope: false);
            ps.Commands.Commands[0].MergeMyResults(PipelineResultTypes.Error, PipelineResultTypes.Output);
            if (capture)
            {
                ps.AddCommand("ForEach-Object").AddParameter("Process", ScriptBlock.Create("$_"));
            }

            ps.AddCommand("Out-Default");
            ps.Invoke();
            return (false, false);
        }
        catch (PipelineStoppedException)
        {
            // ps.Stop() da Ctrl+C (è una RuntimeException: va catturata PRIMA).
            _session.Host.UI.WriteLine("[LARE] comando interrotto (Ctrl+C).");
            return (true, false);
        }
        catch (RuntimeException ex)
        {
            // ParseException (sintassi: fallisce prima di partire, non passa dal merge) e gli
            // errori terminanti fuori pipeline: mostrati e registrati come errore.
            _session.Host.UI.WriteErrorLine(ex.Message);
            return (false, true);
        }
        finally
        {
            _current = null;
        }
    }

    /// <summary>Regola (ruling 3): 0 se il $? catturato dallo script è vero; altrimenti
    /// $LASTEXITCODE se ≠ 0, altrimenti 1. Variabile assente = il comando non è arrivato in fondo
    /// (errore terminante dentro lo script) → non ok. Pulisce il segnaposto.</summary>
    private static int ReadExitCode(SessionStateProxy state)
    {
        bool ok = state.GetVariable(OkVariable) is bool b && b;
        int code = state.GetVariable("LASTEXITCODE") is int c ? c : 0;
        state.PSVariable.Remove(OkVariable);
        return ok ? 0 : code != 0 ? code : 1;
    }
}
```

**Se `Programma_nativo…` o `Capture_false…` restano rossi** (il `$?` catturato in coda allo script non
riflette il nativo fallito), riporta l'output esatto e fermati con **BLOCKED**: è l'assunzione da
verificare di questo task, non va aggirata con un altro meccanismo inventato sul momento.

- [ ] **Step 9: Verifica che passi (GREEN) e commit**

```powershell
dotnet test shell/lare-shell/LareShell.sln
```

Atteso: tutti verdi (i 10 dell'Executor inclusi; `StopCurrent…` dura ≈ 0,5 s).

```powershell
git add shell/lare-shell
git commit -m "feat(lare-shell): runspace ospitata (PSReadLine da pwsh, policy da file), profili con \$PROFILE, Executor (capture, exit code, cwd, Stop)"
```

### Task 5: Gate `[Y/n]` e `SlashTurn` — il ciclo di un turno sul thread del REPL

**Modello: Opus** (concorrenza fra thread del socket, thread del REPL e Ctrl+C).

**Files:**
- Create: `shell/lare-shell/src/LareShell/Shell/Gate.cs`, `SlashTurn.cs`
- Test: `shell/lare-shell/tests/LareShell.Tests/Shell/SlashTurnTests.cs`

**Interfaces:**
- Consumes: `OrchestratorClient` (`Incoming`, `Send`, `IsConnected`), `Wire`, record `ServerMessage` (Task 1–2);
  `IExecutor`, `ExecOutcome` (Task 4); `HostLog` (Task 0); `FakeOrchestrator` (test, Task 2).
- Produces: `enum GateAnswer { Accept, Reject, Cancel, Abandoned }`; `interface IGate { GateAnswer Ask(string commands,
  Func<bool> shouldAbandon); }`; `ConsoleGate : IGate`; `enum TurnResult { Completed, Failed, Cancelled, Disconnected }`;
  `SlashTurn(OrchestratorClient client, IGate gate, IExecutor executor, TextWriter output, HostLog log)` con
  `Func<string> NewId { get; init; }` e `TurnResult Run(string input, string cwd, CancellationToken ctrlC)`.

Sequenza (spec §4.2) e regole (Global Constraints: contratti (a)–(d), gate prima di tutto, Ctrl+C):

```
Run(input, cwd, ctrlC):
  scarta i messaggi rimasti in coda da turni chiusi (ruling 5)
  id = NewId()                               ← GUID: contratto (b)
  Send(Command{id, input, cwd})              ← fallito → "orchestratore non raggiungibile", Disconnected
  loop: msg = Incoming.ReadAsync(ctrlC) (bloccante; ctrlC → Send(CancelCommand{id}), riga, Cancelled)
    Chunk{id==id}             → stampa content
    Done{id==id}              → Completed          ┐ contratto (c): il turno finisce al PRIMO dei due;
    TurnError{id==id}         → stampa, Failed     ┘ tutto ciò che arriva dopo per quell'id è scartato dal prossimo Run
    ToolConfirmRequest        → gate.Ask(commands, shouldAbandon = "in coda c'è già Done/Error del turno o Disconnected")
                                 Accept → ToolConfirmResponse{true}; Reject → {false}; Cancel → CancelCommand, Cancelled; Abandoned → niente
    ExecInShell{TurnId==id}   → executor.Run(command, capture); Stopped → CancelCommand, Cancelled (mai ExecResult parziale)
                                 altrimenti Send(ExecResult{turn_id = ex.TurnId (eco, contratto (d)), exec_id, exit_code, output, cwd})
    ExecInShell{TurnId!=id}   → NON eseguito, log warn (§8: solo ciò che appartiene al turno gateizzato)
    Disconnected              → riga d'errore, Disconnected
    Heartbeat/Pong/ServerInfo → ignorati; Unknown e messaggi di altri turni → log debug
```

- [ ] **Step 1: Scrivi i test (RED)**

`tests/LareShell.Tests/Shell/SlashTurnTests.cs`:

```csharp
using System.Text.Json;
using System.Text.Json.Nodes;
using LareShell.Config;
using LareShell.Protocol;
using LareShell.Shell;
using LareShell.Tests.Protocol;
using Xunit;

namespace LareShell.Tests.Shell;

internal sealed class FakeGate : IGate
{
    public Queue<GateAnswer> Answers { get; } = new();
    public List<string> Asked { get; } = new();
    public List<GateAnswer> Given { get; } = new();

    public GateAnswer Ask(string commands, Func<bool> shouldAbandon)
    {
        Asked.Add(commands);
        // Senza risposta pronta simula l'utente che non preme nulla: aspetta che il turno finisca.
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (Answers.Count == 0)
        {
            if (shouldAbandon())
            {
                Given.Add(GateAnswer.Abandoned);
                return GateAnswer.Abandoned;
            }

            if (DateTime.UtcNow > deadline) throw new TimeoutException("il gate finto non ha ricevuto né risposta né abbandono");
            Thread.Sleep(10);
        }

        GateAnswer a = Answers.Dequeue();
        Given.Add(a);
        return a;
    }
}

internal sealed class FakeExecutor : IExecutor
{
    public List<(string Command, bool Capture, int ThreadId)> Calls { get; } = new();
    public ExecOutcome Outcome { get; set; } = new(0, "fake-output", @"C:\x", Stopped: false);

    public ExecOutcome Run(string command, bool capture)
    {
        Calls.Add((command, capture, Environment.CurrentManagedThreadId));
        return Outcome;
    }

    public void StopCurrent() { }
}

public class SlashTurnTests
{
    private static CancellationToken Ct() => new CancellationTokenSource(TimeSpan.FromSeconds(10)).Token;

    private static string J(object o) => JsonSerializer.Serialize(o);

    /// <summary>Server finto + client vero già connessi (handshake fatto).</summary>
    private static (FakeOrchestrator Server, OrchestratorClient Client) Connected()
    {
        FakeOrchestrator server = FakeOrchestrator.Start();
        var client = new OrchestratorClient(server.Uri, () => "tok", "s1", "2.0.0", HostLog.Null);
        Task<JsonObject> accepted = server.AcceptAsync(Ct());
        string? reason = client.Connect(@"C:\w", TimeSpan.FromSeconds(10));
        accepted.GetAwaiter().GetResult();
        Assert.Null(reason);
        return (server, client);
    }

    [Fact]
    public void Turno_completo_gate_exec_chunk_done()
    {
        // spec §4.2 + §10: Hello→Command→ToolConfirmRequest→ToolConfirmResponse→ExecInShell→ExecResult→Done
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();
            gate.Answers.Enqueue(GateAnswer.Accept);
            var executor = new FakeExecutor();
            var output = new StringWriter();
            var turn = new SlashTurn(client, gate, executor, output, HostLog.Null) { NewId = () => "turno-1" };
            CancellationToken ct = Ct();

            Task<(JsonObject Cmd, JsonObject Confirm, JsonObject Result)> serverSide = Task.Run(async () =>
            {
                JsonObject cmd = await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "Get-Date" }), ct);
                JsonObject confirm = await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "exec_in_shell", turn_id = "turno-1", exec_id = "e1", command = "Get-Date", capture = true }), ct);
                JsonObject result = await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "chunk", id = "turno-1", content = "→ finestra \"x\" aperta" }), ct);
                await server.SendAsync(J(new { type = "done", id = "turno-1", exit_code = (int?)null }), ct);
                return (cmd, confirm, result);
            });

            int replThread = Environment.CurrentManagedThreadId;
            TurnResult r = turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None);
            (JsonObject cmd, JsonObject confirm, JsonObject result) = serverSide.GetAwaiter().GetResult();

            Assert.Equal(TurnResult.Completed, r);
            Assert.Equal("command", (string?)cmd["type"]);
            Assert.Equal("turno-1", (string?)cmd["id"]);
            Assert.Equal("/ai \"x\"", (string?)cmd["input"]);
            Assert.Equal(@"C:\w", (string?)cmd["cwd"]);
            Assert.Equal("tool_confirm_response", (string?)confirm["type"]);
            Assert.Equal("g1", (string?)confirm["id"]);
            Assert.True((bool?)confirm["accept"]);
            Assert.Equal(new[] { "Get-Date" }, gate.Asked);
            // exec_result: eco del turn_id (contratto d), exec_id, esito del finto executor
            Assert.Equal("exec_result", (string?)result["type"]);
            Assert.Equal("turno-1", (string?)result["turn_id"]);
            Assert.Equal("e1", (string?)result["exec_id"]);
            Assert.Equal(0, (int?)result["exit_code"]);
            Assert.Equal("fake-output", (string?)result["output"]);
            Assert.Equal(@"C:\x", (string?)result["cwd"]);
            // marshaling (spec §10): l'ExecInShell arrivato dal thread del socket è eseguito dal thread che ha chiamato Run
            (string command, bool capture, int threadId) = Assert.Single(executor.Calls);
            Assert.Equal(("Get-Date", true, replThread), (command, capture, threadId));
            Assert.Contains("→ finestra \"x\" aperta", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Rifiuto_al_gate_manda_accept_false_e_non_esegue_nulla()
    {
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();
            gate.Answers.Enqueue(GateAnswer.Reject);
            var executor = new FakeExecutor();
            var turn = new SlashTurn(client, gate, executor, TextWriter.Null, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();

            Task<JsonObject> serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "Remove-Item x" }), ct);
                JsonObject confirm = await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "done", id = "t", exit_code = (int?)null }), ct);
                return confirm;
            });

            Assert.Equal(TurnResult.Completed, turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None));
            Assert.False((bool?)serverSide.GetAwaiter().GetResult()["accept"]);
            Assert.Empty(executor.Calls);
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Ctrl_C_in_attesa_manda_cancel_command_e_torna_Cancelled()
    {
        // spec §4.4: "Durante l'attesa del turno: la host manda CancelCommand{id}, stampa una riga, torna al prompt"
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var output = new StringWriter();
            var turn = new SlashTurn(client, new FakeGate(), new FakeExecutor(), output, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            using var ctrlC = new CancellationTokenSource();

            Task<JsonObject> serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);          // command: l'orchestratore "ci pensa"…
                return await server.ReceiveAsync(ct);   // …e riceve il cancel
            });
            Task.Run(async () => { await Task.Delay(200); ctrlC.Cancel(); });

            Assert.Equal(TurnResult.Cancelled, turn.Run("/ai \"x\"", @"C:\w", ctrlC.Token));
            JsonObject cancel = serverSide.GetAwaiter().GetResult();
            Assert.Equal("cancel_command", (string?)cancel["type"]);
            Assert.Equal("t", (string?)cancel["id"]);
            Assert.Contains("annullato", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Error_chiude_il_turno_con_Failed_e_stampa_il_messaggio()
    {
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var output = new StringWriter();
            var turn = new SlashTurn(client, new FakeGate(), new FakeExecutor(), output, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "error", id = "t", code = "routing_error", message = "sintassi: /ai \"testo\"" }), ct);
            });

            Assert.Equal(TurnResult.Failed, turn.Run("/ai x", @"C:\w", CancellationToken.None));
            serverSide.GetAwaiter().GetResult();
            Assert.Contains("sintassi: /ai \"testo\"", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Messaggi_di_altri_turni_vengono_scartati()
    {
        // contratto (c): un Done di un turno già chiuso/diverso non chiude quello corrente
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var executor = new FakeExecutor();
            var turn = new SlashTurn(client, new FakeGate(), executor, TextWriter.Null, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "done", id = "vecchio", exit_code = (int?)null }), ct);
                await server.SendAsync(J(new { type = "exec_in_shell", turn_id = "altro", exec_id = "e9", command = "Remove-Item x", capture = true }), ct);
                await server.SendAsync(J(new { type = "heartbeat", id = "t" }), ct);
                await server.SendAsync(J(new { type = "done", id = "t", exit_code = (int?)null }), ct);
            });

            Assert.Equal(TurnResult.Completed, turn.Run("/ping", @"C:\w", CancellationToken.None));
            serverSide.GetAwaiter().GetResult();
            Assert.Empty(executor.Calls);   // §8: l'ExecInShell di un altro turno NON viene eseguito
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Exec_fermato_da_Ctrl_C_cancella_il_turno_senza_ExecResult()
    {
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();
            gate.Answers.Enqueue(GateAnswer.Accept);
            var executor = new FakeExecutor { Outcome = new ExecOutcome(130, "", @"C:\w", Stopped: true) };
            var turn = new SlashTurn(client, gate, executor, TextWriter.Null, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task<JsonObject> serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "Start-Sleep 99" }), ct);
                await server.ReceiveAsync(ct);   // accept
                await server.SendAsync(J(new { type = "exec_in_shell", turn_id = "t", exec_id = "e1", command = "Start-Sleep 99", capture = true }), ct);
                return await server.ReceiveAsync(ct);   // deve essere cancel_command, NON exec_result
            });

            Assert.Equal(TurnResult.Cancelled, turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None));
            Assert.Equal("cancel_command", (string?)serverSide.GetAwaiter().GetResult()["type"]);
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Caduta_della_connessione_a_meta_turno_torna_Disconnected()
    {
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var output = new StringWriter();
            var turn = new SlashTurn(client, new FakeGate(), new FakeExecutor(), output, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.CloseAsync();
            });

            Assert.Equal(TurnResult.Disconnected, turn.Run("/ping", @"C:\w", CancellationToken.None));
            serverSide.GetAwaiter().GetResult();
            Assert.Contains("orchestratore", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Il_gate_viene_abbandonato_se_in_coda_c_e_gia_il_Done_del_turno()
    {
        // Timeout 180 s lato orchestratore (spec §4.3): la richiesta scade e arriva Done/Error mentre
        // l'utente non ha ancora risposto → il prompt si chiude da solo, senza restare appeso.
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();   // nessuna risposta in coda: aspetta shouldAbandon
            var turn = new SlashTurn(client, gate, new FakeExecutor(), TextWriter.Null, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "x" }), ct);
                await server.SendAsync(J(new { type = "error", id = "t", code = "ai_error", message = "conferma scaduta" }), ct);
            });

            Assert.Equal(TurnResult.Failed, turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None));
            serverSide.GetAwaiter().GetResult();
            Assert.Equal(new[] { GateAnswer.Abandoned }, gate.Given);
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Senza_connessione_Run_torna_Disconnected_senza_bloccare()
    {
        using FakeOrchestrator server = FakeOrchestrator.Start();
        using var client = new OrchestratorClient(server.Uri, () => "tok", "s1", "2.0.0", HostLog.Null);   // mai connesso
        var output = new StringWriter();
        var turn = new SlashTurn(client, new FakeGate(), new FakeExecutor(), output, HostLog.Null);
        Assert.Equal(TurnResult.Disconnected, turn.Run("/ping", @"C:\w", CancellationToken.None));
        Assert.Contains("non raggiungibile", output.ToString());
    }
}
```

Nota per l'implementer: `FakeOrchestrator` implementa `IAsyncDisposable`; i test di `SlashTurn` sono
sincroni (`Run` blocca il thread chiamante, come il REPL), quindi il server si chiude in coda con
`server.DisposeAsync().AsTask().GetAwaiter().GetResult()`. Non convertirli in `async Task`.

- [ ] **Step 2: Verifica che fallisca (RED)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~SlashTurn"
```

Atteso: `CS0246` su `SlashTurn`/`IGate`/`GateAnswer`/`TurnResult`.

- [ ] **Step 3: Implementa `Gate.cs`**

```csharp
namespace LareShell.Shell;

/// <summary>Risposta dell'utente al gate ADR-007 (spec §4.3). <c>Cancel</c> = Ctrl+C (annulla tutto
/// il turno, non solo questa richiesta); <c>Abandoned</c> = il turno è finito mentre aspettavamo.</summary>
internal enum GateAnswer
{
    Accept,
    Reject,
    Cancel,
    Abandoned,
}

/// <summary>Confine d'astrazione del prompt [Y/n]: SlashTurn ne dipende, i test passano un finto.</summary>
internal interface IGate
{
    /// <param name="shouldAbandon">Interrogato a ogni giro di attesa: true = smetti di aspettare.</param>
    GateAnswer Ask(string commands, Func<bool> shouldAbandon);
}

/// <summary>
/// Prompt [Y/n] nel terminale con lettura tasto diretta (spec §4.3: "non PSReadLine"). Non blocca
/// in Console.ReadKey: interroga <c>KeyAvailable</c> ogni 50 ms, così può accorgersi (via
/// <paramref name="shouldAbandon"/>) che il turno è già finito. Durante il prompt Ctrl+C arriva
/// come TASTO (TreatControlCAsInput) e vale come annullamento del turno.
/// Invio o Y/y/S/s = accetta (default [Y/n]); N/n = rifiuta.
/// </summary>
internal sealed class ConsoleGate : IGate
{
    public GateAnswer Ask(string commands, Func<bool> shouldAbandon)
    {
        Console.WriteLine();
        Console.ForegroundColor = ConsoleColor.Yellow;
        Console.WriteLine("L'AI propone di eseguire:");
        Console.ResetColor();
        foreach (string line in commands.Replace("\r\n", "\n").Split('\n'))
        {
            Console.WriteLine("  " + line);
        }

        Console.Write("Eseguire? [Y/n] ");

        if (Console.IsInputRedirected)
        {
            // Nessuna tastiera (pipe/test manuale): una riga di testo.
            string? line = Console.ReadLine();
            return line is null or "" || line.StartsWith('y') || line.StartsWith('Y') || line.StartsWith('s') || line.StartsWith('S')
                ? GateAnswer.Accept
                : GateAnswer.Reject;
        }

        bool previous = false;
        try { previous = Console.TreatControlCAsInput; Console.TreatControlCAsInput = true; } catch { /* nessuna console */ }
        try
        {
            while (!shouldAbandon())
            {
                if (!Console.KeyAvailable)
                {
                    Thread.Sleep(50);
                    continue;
                }

                ConsoleKeyInfo key = Console.ReadKey(intercept: true);
                if (key.Key == ConsoleKey.C && key.Modifiers.HasFlag(ConsoleModifiers.Control))
                {
                    Console.WriteLine("^C");
                    return GateAnswer.Cancel;
                }

                if (key.Key == ConsoleKey.Enter || key.KeyChar is 'y' or 'Y' or 's' or 'S')
                {
                    Console.WriteLine("y");
                    return GateAnswer.Accept;
                }

                if (key.KeyChar is 'n' or 'N')
                {
                    Console.WriteLine("n");
                    return GateAnswer.Reject;
                }
            }

            Console.WriteLine();
            Console.WriteLine("(richiesta scaduta: il turno è terminato)");
            return GateAnswer.Abandoned;
        }
        finally
        {
            try { Console.TreatControlCAsInput = previous; } catch { /* nessuna console */ }
        }
    }
}
```

- [ ] **Step 4: Implementa `SlashTurn.cs`**

```csharp
using LareShell.Config;
using LareShell.Protocol;

namespace LareShell.Shell;

internal enum TurnResult
{
    Completed,
    Failed,
    Cancelled,
    Disconnected,
}

/// <summary>
/// Il ciclo di UN turno slash (spec §4.2), eseguito interamente sul thread del REPL: manda il
/// Command, poi consuma il canale dei messaggi in arrivo finché il turno non finisce. Il thread
/// del socket ha già accodato tutto in <c>OrchestratorClient.Incoming</c>; qui si consuma e si
/// reagisce — gate, esecuzione nel runspace, stampa — sempre da questo thread (§4.4).
/// Contratti (crates/protocol/IMPLEMENTATION.md): (a) un turno alla volta — garantito perché Run
/// blocca il REPL; (b) id = GUID; (c) fine al primo Done/Error; (d) turn_id echeggiato.
/// </summary>
internal sealed class SlashTurn
{
    private readonly OrchestratorClient _client;
    private readonly IGate _gate;
    private readonly IExecutor _executor;
    private readonly TextWriter _output;
    private readonly HostLog _log;

    public SlashTurn(OrchestratorClient client, IGate gate, IExecutor executor, TextWriter output, HostLog log)
    {
        _client = client;
        _gate = gate;
        _executor = executor;
        _output = output;
        _log = log;
    }

    /// <summary>Generatore dell'id del turno (contratto b). Sostituibile nei test.</summary>
    public Func<string> NewId { get; init; } = () => Guid.NewGuid().ToString("N");

    public TurnResult Run(string input, string cwd, CancellationToken ctrlC)
    {
        DiscardStale();

        string id = NewId();
        if (!_client.IsConnected || !_client.Send(Wire.Command(id, input, cwd)))
        {
            _output.WriteLine("orchestratore non raggiungibile: comando ignorato");
            return TurnResult.Disconnected;
        }

        _log.Info("turno " + id + " avviato: " + input);

        while (true)
        {
            ServerMessage msg;
            try
            {
                // Ponte bloccante sul canale: il thread del REPL dorme finché il socket non accoda
                // qualcosa o l'utente preme Ctrl+C (token cancellato dal Repl).
                msg = _client.Incoming.ReadAsync(ctrlC).AsTask().GetAwaiter().GetResult();
            }
            catch (OperationCanceledException)
            {
                return Cancel(id, "annullato (Ctrl+C)");
            }

            switch (msg)
            {
                case Chunk c when c.Id == id:
                    _output.WriteLine(c.Content);
                    break;

                case Done d when d.Id == id:
                    _log.Info("turno " + id + " completato");
                    return TurnResult.Completed;

                case TurnError e when e.Id == id:
                    _output.WriteLine("errore: " + e.Message);
                    _log.Warn("turno " + id + " in errore (" + e.Code + "): " + e.Message);
                    return TurnResult.Failed;

                case ToolConfirmRequest req:
                    // Attribuita al turno corrente (ruling 5: l'id della richiesta è opaco, ma un
                    // solo turno alla volta è in corso — contratto a).
                    switch (_gate.Ask(req.Commands, () => TerminalPending(id)))
                    {
                        case GateAnswer.Accept:
                            _client.Send(Wire.ToolConfirmResponse(req.Id, accept: true));
                            break;
                        case GateAnswer.Reject:
                            _client.Send(Wire.ToolConfirmResponse(req.Id, accept: false));
                            break;
                        case GateAnswer.Cancel:
                            return Cancel(id, "annullato (Ctrl+C)");
                        case GateAnswer.Abandoned:
                            break;   // il Done/Error in coda chiuderà il turno al prossimo giro
                    }

                    break;

                case ExecInShell ex when ex.TurnId == id:
                    ExecOutcome outcome = _executor.Run(ex.Command, ex.Capture);
                    if (outcome.Stopped)
                    {
                        // §4.4: Ctrl+C durante l'exec = stop della pipeline E cancel del turno, mai un ExecResult parziale.
                        return Cancel(id, "comando interrotto (Ctrl+C): turno annullato");
                    }

                    // (d) eco del turn_id ricevuto, mai il nostro.
                    _client.Send(Wire.ExecResult(ex.TurnId, ex.ExecId, outcome.ExitCode, outcome.Output, outcome.Cwd));
                    break;

                case ExecInShell other:
                    // §8: si esegue SOLO ciò che appartiene al turno gateizzato in corso.
                    _log.Warn("ExecInShell per un altro turno (" + other.TurnId + " ≠ " + id + "): NON eseguito");
                    break;

                case Disconnected dc:
                    _output.WriteLine("connessione all'orchestratore persa (" + dc.Reason + "): turno interrotto");
                    return TurnResult.Disconnected;

                case Heartbeat or Pong or ServerInfo:
                    break;

                default:
                    _log.Debug("messaggio scartato durante il turno " + id + ": " + msg);
                    break;
            }
        }
    }

    private TurnResult Cancel(string id, string line)
    {
        _client.Send(Wire.CancelCommand(id));
        _output.WriteLine(line);
        _log.Info("turno " + id + " annullato");
        return TurnResult.Cancelled;
    }

    /// <summary>true se in TESTA alla coda c'è già la fine del turno (Done/Error con questo id) o
    /// la caduta della connessione: il gate smette di aspettare l'utente. Limite noto: TryPeek vede
    /// solo il primo messaggio — se prima del Done c'è un Chunk (l'ack di conferma), il prompt resta
    /// finché l'utente non preme un tasto; poi il turno si chiude normalmente.</summary>
    private bool TerminalPending(string id) =>
        _client.Incoming.TryPeek(out ServerMessage? next) &&
        (next is Done d && d.Id == id || next is TurnError e && e.Id == id || next is Disconnected);

    /// <summary>Scarta ciò che è rimasto in coda da turni già chiusi (contratto c, ruling 5).
    /// Un Disconnected non va "perso": IsConnected lo rende comunque visibile al chiamante.</summary>
    private void DiscardStale()
    {
        while (_client.Incoming.TryRead(out ServerMessage? stale))
        {
            _log.Debug("scartato fuori turno: " + stale);
        }
    }
}
```

- [ ] **Step 5: Verifica che passi (GREEN)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~SlashTurn"
```

Atteso: 9 test superati in < 10 s. Un test che resta appeso = un messaggio non consumato o un
`Send` su socket non aperto: cerca lì, non nei timeout.

- [ ] **Step 6: Commit**

```powershell
git add shell/lare-shell
git commit -m "feat(lare-shell): gate [Y/n] con polling dei tasti e SlashTurn (ciclo del turno sul thread del REPL, contratti a-d)"
```

### Task 6: `Launcher` — autostart di `orchestrator.exe` (retry 5 s) e `ui.exe` (§6.4)

**Files:**
- Create: `shell/lare-shell/src/LareShell/Shell/Launcher.cs`
- Test: `shell/lare-shell/tests/LareShell.Tests/Shell/LauncherTests.cs`

**Interfaces:**
- Consumes: `StartupConfig`, `HostLog` (Task 0).
- Produces: `interface IProcessStarter { bool Exists(string exePath); bool IsRunning(string exePath); void Start(string exePath,
  IReadOnlyList<string> args, bool hideWindow); }`; `ProcessStarter : IProcessStarter`;
  `Launcher(string deployRoot, string configDir, StartupConfig cfg, IProcessStarter starter, HostLog log, TextWriter console)`
  con `OrchestratorExe`, `UiExe`, `bool EnsureConnected(Func<string?> connect, TimeSpan window, TimeSpan retry)`
  (default `ConnectWindow = 5 s`, `RetryInterval = 250 ms`), `bool EnsureUi()`.

- [ ] **Step 1: Scrivi i test (RED)**

`tests/LareShell.Tests/Shell/LauncherTests.cs`:

```csharp
using LareShell.Config;
using LareShell.Shell;
using Xunit;

namespace LareShell.Tests.Shell;

internal sealed class FakeStarter : IProcessStarter
{
    public HashSet<string> Existing { get; } = new(StringComparer.OrdinalIgnoreCase);
    public HashSet<string> Running { get; } = new(StringComparer.OrdinalIgnoreCase);
    public List<(string Exe, IReadOnlyList<string> Args, bool Hidden)> Started { get; } = new();

    public bool Exists(string exePath) => Existing.Contains(exePath);

    public bool IsRunning(string exePath) => Running.Contains(exePath);

    public void Start(string exePath, IReadOnlyList<string> args, bool hideWindow) => Started.Add((exePath, args, hideWindow));
}

public class LauncherTests
{
    private const string Root = @"C:\Lare";
    private const string Cfg = @"C:\Lare\Configuration";
    private static readonly TimeSpan Window = TimeSpan.FromMilliseconds(300);
    private static readonly TimeSpan Retry = TimeSpan.FromMilliseconds(20);

    private static (Launcher L, FakeStarter S, StringWriter Out) Make(bool autoOrch = true, bool autoUi = true)
    {
        var starter = new FakeStarter();
        starter.Existing.Add(@"C:\Lare\orchestrator.exe");
        starter.Existing.Add(@"C:\Lare\ui.exe");
        var output = new StringWriter();
        var cfg = new StartupConfig { AutostartOrchestrator = autoOrch, AutostartUi = autoUi };
        return (new Launcher(Root, Cfg, cfg, starter, HostLog.Null, output), starter, output);
    }

    /// <summary>Sequenza di esiti di connessione: null = connesso.</summary>
    private static Func<string?> Sequence(params string?[] outcomes)
    {
        int i = 0;
        return () => i < outcomes.Length ? outcomes[i++] : outcomes[^1];
    }

    [Fact]
    public void Percorsi_degli_eseguibili_nella_radice_del_deploy()
    {
        (Launcher l, _, _) = Make();
        Assert.Equal(@"C:\Lare\orchestrator.exe", l.OrchestratorExe);
        Assert.Equal(@"C:\Lare\ui.exe", l.UiExe);
    }

    [Fact]
    public void Se_la_connessione_riesce_subito_non_avvia_nulla()
    {
        (Launcher l, FakeStarter s, _) = Make();
        Assert.True(l.EnsureConnected(Sequence((string?)null), Window, Retry));
        Assert.Empty(s.Started);
    }

    [Fact]
    public void Se_fallisce_avvia_l_orchestratore_con_config_dir_e_ritenta_finche_riesce()
    {
        // spec §6.4: "non raggiungono il WS → se autostart.orchestrator, avviano orchestrator.exe e ritentano per 5 s"
        (Launcher l, FakeStarter s, StringWriter o) = Make();
        Assert.True(l.EnsureConnected(Sequence("rifiutata", "rifiutata", null), Window, Retry));
        (string exe, IReadOnlyList<string> args, bool hidden) = Assert.Single(s.Started);
        Assert.Equal(@"C:\Lare\orchestrator.exe", exe);
        Assert.Equal(new[] { "--config-dir", Cfg }, args);   // D6: i figli ricevono --config-dir per argomento
        Assert.True(hidden);                                  // app console: nessuna finestra
        Assert.Contains("avvio", o.ToString(), StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void Con_autostart_disattivo_non_avvia_e_torna_false()
    {
        (Launcher l, FakeStarter s, StringWriter o) = Make(autoOrch: false);
        Assert.False(l.EnsureConnected(Sequence("rifiutata"), Window, Retry));
        Assert.Empty(s.Started);
        Assert.Contains("autostart", o.ToString(), StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void Se_l_eseguibile_manca_non_avvia_e_lo_dice()
    {
        (Launcher l, FakeStarter s, StringWriter o) = Make();
        s.Existing.Remove(@"C:\Lare\orchestrator.exe");
        Assert.False(l.EnsureConnected(Sequence("rifiutata"), Window, Retry));
        Assert.Empty(s.Started);
        Assert.Contains(@"C:\Lare\orchestrator.exe", o.ToString());
    }

    [Fact]
    public void Se_non_si_connette_entro_la_finestra_torna_false_dopo_un_solo_avvio()
    {
        (Launcher l, FakeStarter s, StringWriter o) = Make();
        var sw = System.Diagnostics.Stopwatch.StartNew();
        Assert.False(l.EnsureConnected(Sequence("rifiutata"), Window, Retry));
        Assert.Single(s.Started);
        Assert.True(sw.Elapsed >= Window, "ha smesso di ritentare troppo presto: " + sw.Elapsed);
        Assert.Contains("non raggiungibile", o.ToString());
    }

    [Fact]
    public void EnsureUi_avvia_ui_exe_solo_se_non_gira_gia()
    {
        (Launcher l, FakeStarter s, _) = Make();
        Assert.True(l.EnsureUi());
        (string exe, IReadOnlyList<string> args, bool hidden) = Assert.Single(s.Started);
        Assert.Equal(@"C:\Lare\ui.exe", exe);
        Assert.Equal(new[] { "--config-dir", Cfg }, args);
        Assert.False(hidden);   // app GUI: finestra normale

        s.Running.Add(@"C:\Lare\ui.exe");
        Assert.False(l.EnsureUi());
        Assert.Single(s.Started);
    }

    [Fact]
    public void EnsureUi_rispetta_autostart_ui_e_l_assenza_dell_exe()
    {
        (Launcher l1, FakeStarter s1, _) = Make(autoUi: false);
        Assert.False(l1.EnsureUi());
        Assert.Empty(s1.Started);

        (Launcher l2, FakeStarter s2, _) = Make();
        s2.Existing.Remove(@"C:\Lare\ui.exe");
        Assert.False(l2.EnsureUi());
        Assert.Empty(s2.Started);
    }
}
```

- [ ] **Step 2: Verifica che fallisca (RED)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Launcher"
```

Atteso: `CS0246` su `Launcher`/`IProcessStarter`.

- [ ] **Step 3: Implementa `Launcher.cs`**

```csharp
using System.Diagnostics;
using LareShell.Config;

namespace LareShell.Shell;

/// <summary>Confine d'astrazione sui processi: il Launcher decide COSA avviare, questo COME.</summary>
internal interface IProcessStarter
{
    bool Exists(string exePath);

    /// <summary>true se un processo con quel percorso esatto di eseguibile è già vivo.</summary>
    bool IsRunning(string exePath);

    void Start(string exePath, IReadOnlyList<string> args, bool hideWindow);
}

/// <summary>
/// Avvio "staccato" (spec §6.4): <c>UseShellExecute = true</c> → il figlio NON eredita gli handle
/// della nostra console (stdio compresi) e vive in una console propria: un Ctrl+C nella shell non
/// lo abbatte, e i suoi log non sporcano il terminale. <c>WindowStyle = Hidden</c> per le app
/// console (orchestrator.exe) ne nasconde la finestra; ui.exe è un'app GUI e va lasciata Normal
/// (ruling 4 del piano — è l'equivalente pratico in .NET di DETACHED_PROCESS).
/// </summary>
internal sealed class ProcessStarter : IProcessStarter
{
    public bool Exists(string exePath) => File.Exists(exePath);

    public bool IsRunning(string exePath)
    {
        string name = Path.GetFileNameWithoutExtension(exePath);
        foreach (Process p in Process.GetProcessesByName(name))
        {
            try
            {
                // MainModule lancia per i processi di altri utenti/elevati: quelli non sono "il nostro" ui.exe.
                if (string.Equals(p.MainModule?.FileName, exePath, StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
            }
            catch (Exception ex) when (ex is System.ComponentModel.Win32Exception or InvalidOperationException)
            {
                // ignorato: vedi sopra
            }
            finally
            {
                p.Dispose();
            }
        }

        return false;
    }

    public void Start(string exePath, IReadOnlyList<string> args, bool hideWindow)
    {
        var psi = new ProcessStartInfo(exePath)
        {
            UseShellExecute = true,
            WorkingDirectory = Path.GetDirectoryName(exePath) ?? string.Empty,
            WindowStyle = hideWindow ? ProcessWindowStyle.Hidden : ProcessWindowStyle.Normal,
        };
        foreach (string a in args)
        {
            psi.ArgumentList.Add(a);
        }

        Process.Start(psi)?.Dispose();
    }
}

/// <summary>
/// Self-heal all'avvio della host in modalità B (spec §6.4): se il WS non risponde e
/// <c>autostart.orchestrator</c> è attivo, avvia <c>&lt;radice deploy&gt;\orchestrator.exe --config-dir …</c>
/// e ritenta per 5 s; poi, se <c>autostart.ui</c>, avvia <c>ui.exe</c> quando non c'è già un
/// processo con quel percorso. Gara all'avvio (§2.3): due host che partono insieme avviano due
/// orchestratori; il bind della porta decide (il secondo esce), entrambe le host si connettono
/// entro la finestra di retry. Stampa sul <c>console</c> passato (mai su Console direttamente: testabile).
/// </summary>
internal sealed class Launcher
{
    public static readonly TimeSpan ConnectWindow = TimeSpan.FromSeconds(5);
    public static readonly TimeSpan RetryInterval = TimeSpan.FromMilliseconds(250);

    private readonly string _configDir;
    private readonly StartupConfig _cfg;
    private readonly IProcessStarter _starter;
    private readonly HostLog _log;
    private readonly TextWriter _console;

    public Launcher(string deployRoot, string configDir, StartupConfig cfg, IProcessStarter starter, HostLog log, TextWriter console)
    {
        OrchestratorExe = Path.Combine(deployRoot, "orchestrator.exe");
        UiExe = Path.Combine(deployRoot, "ui.exe");
        _configDir = configDir;
        _cfg = cfg;
        _starter = starter;
        _log = log;
        _console = console;
    }

    public string OrchestratorExe { get; }

    public string UiExe { get; }

    /// <param name="connect">Un tentativo di connessione: null = riuscito, altrimenti il motivo.</param>
    public bool EnsureConnected(Func<string?> connect, TimeSpan window, TimeSpan retry)
    {
        string? reason = connect();
        if (reason is null)
        {
            return true;
        }

        if (!_cfg.AutostartOrchestrator)
        {
            _console.WriteLine("orchestratore non raggiungibile (" + reason + "); autostart disattivo in startup.json");
            return false;
        }

        if (!_starter.Exists(OrchestratorExe))
        {
            _console.WriteLine("orchestratore non raggiungibile (" + reason + ") e " + OrchestratorExe + " non esiste");
            return false;
        }

        _console.WriteLine("orchestratore non raggiungibile (" + reason + "): avvio " + OrchestratorExe);
        _log.Info("autostart orchestratore: " + OrchestratorExe);
        _starter.Start(OrchestratorExe, new[] { "--config-dir", _configDir }, hideWindow: true);

        var deadline = DateTime.UtcNow + window;
        while (DateTime.UtcNow < deadline)
        {
            Thread.Sleep(retry);
            reason = connect();
            if (reason is null)
            {
                _console.WriteLine("orchestratore avviato e connesso");
                return true;
            }
        }

        _console.WriteLine("orchestratore non raggiungibile dopo " + window.TotalSeconds + " s (" + reason + "): i comandi /… non funzioneranno finché non risponde");
        _log.Warn("autostart orchestratore fallito: " + reason);
        return false;
    }

    /// <summary>Avvia ui.exe se manca e l'autostart è attivo. true = avviata adesso.</summary>
    public bool EnsureUi()
    {
        if (!_cfg.AutostartUi || !_starter.Exists(UiExe) || _starter.IsRunning(UiExe))
        {
            return false;
        }

        _log.Info("autostart ui: " + UiExe);
        _starter.Start(UiExe, new[] { "--config-dir", _configDir }, hideWindow: false);
        return true;
    }
}
```

- [ ] **Step 4: Verifica che passi (GREEN) e commit**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Launcher"
```

Atteso: 8 test superati (uno dura ≈ 0,3 s).

```powershell
git add shell/lare-shell
git commit -m "feat(lare-shell): Launcher — autostart di orchestrator.exe con retry 5 s e di ui.exe (processi senza console ereditata)"
```

### Task 7: `SlashLine`, `Osc`, `Repl`, `Program` (con `--selftest`) — la host completa

**Files:**
- Create: `shell/lare-shell/src/LareShell/SlashLine.cs`, `Osc.cs`, `Repl.cs`, `SelfTest.cs`
- Modify: `shell/lare-shell/src/LareShell/Program.cs` (sostituisce il provvisorio del Task 0)
- Test: `shell/lare-shell/tests/LareShell.Tests/ReplTests.cs`

**Interfaces:**
- Consumes: tutto ciò che precede — `CliArgs`, `ConfigDir`, `StartupConfig`, `TokenFile`, `HostLog` (Task 0);
  `OrchestratorClient` (Task 2); `LareHost`, `ConsoleModes.TryEnableVirtualTerminalProcessing` (Task 3);
  `RunspaceSession`, `ProfileLoader`, `Executor` (Task 4); `ConsoleGate`, `SlashTurn`, `TurnResult` (Task 5);
  `Launcher`, `ProcessStarter` (Task 6).
- Produces: `SlashLine.IsSlash(string raw) → bool`, `SlashLine.Normalize(string raw) → string`;
  `Osc.Esc = (char)0x1B`, `Osc.Intercept(string line) → string`;
  `Repl(RunspaceSession, Executor, OrchestratorClient, Launcher, HostLog)` con `int Run()`;
  `SelfTest.Run(string configDir, StartupConfig cfg) → int`; `Program.Main`.

- [ ] **Step 1: Test di `SlashLine`, `Osc` e scansione dei sorgenti (RED)**

`tests/LareShell.Tests/ReplTests.cs`:

```csharp
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
```

- [ ] **Step 2: Verifica che fallisca (RED)**

```powershell
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~SlashLine|FullyQualifiedName~Osc|FullyQualifiedName~SourceScan"
```

Atteso: `CS0246` su `SlashLine`/`Osc` (il test di scansione compila solo con gli altri).

- [ ] **Step 3: Implementa `SlashLine.cs` e `Osc.cs`**

`src/LareShell/SlashLine.cs`:

```csharp
namespace LareShell;

/// <summary>
/// Riconoscimento delle righe da intercettare (spec §1.1: "i comandi slash vengono interpretati
/// dal nostro programma"): una riga la cui prima cosa non-spazio è '/'. Il resto del routing
/// (comando noto/ignoto, /ai con virgolette…) lo fa l'orchestratore, non la host.
/// Funzioni pure: la stessa logica del REPL, testata senza console.
/// </summary>
internal static class SlashLine
{
    public static bool IsSlash(string raw) => raw.TrimStart().StartsWith('/');

    /// <summary>Il testo inviato come <c>Command.input</c>: solo trim ai bordi.</summary>
    public static string Normalize(string raw) => raw.Trim();
}
```

`src/LareShell/Osc.cs`:

```csharp
namespace LareShell;

/// <summary>
/// Canale diretto host → emulatore (spec §4.7): la sequenza OSC 9001 che segnala l'ultimo
/// comando intercettato. In modalità B Windows Terminal la ignora (OSC sconosciuta); in modalità A
/// (piano 3) xterm.js la cattura con registerOscHandler(9001, …). ESC è costruito da intero:
/// MAI con un escape di stringa (backslash, x, 1, b) in C# — l'escape \x è "goloso" e mangia le
/// cifre esadecimali che seguono (bug dello spike 1); SourceScanTests fallisce se ricompare.
/// </summary>
internal static class Osc
{
    internal static readonly char Esc = (char)0x1B;

    public static string Intercept(string line) =>
        Esc + "]9001;lare;intercept;" + Sanitize(line) + Esc + "\\";

    /// <summary>Un ESC, un BEL o un a-capo nel payload chiuderebbero/spezzerebbero la sequenza.</summary>
    private static string Sanitize(string s) => new(s.Where(c => !char.IsControl(c)).ToArray());
}
```

Esegui il filtro dello Step 2: 13 test verdi (9 dalla theory + 1 Normalize + 2 Osc + 1 scansione).

- [ ] **Step 4: Implementa `Repl.cs`**

```csharp
using LareShell.Config;
using LareShell.Protocol;
using LareShell.Shell;

namespace LareShell;

/// <summary>
/// Il ciclo read-eval-print-loop (come Repl.cs dello spike, senza barre VT — D13): prompt
/// dell'utente → riga (PSReadLine) → se inizia con '/' va all'orchestratore (SlashTurn), altrimenti
/// al runspace (Executor.RunInteractive). Tutto su QUESTO thread (§4.4). Ctrl+C: ferma la
/// pipeline in corso e cancella il turno slash in corso (token), come pwsh — non esce dalla shell.
/// Riconnessione on demand (ruling 2): prima di ogni /… si verifica la connessione e, se manca,
/// la si ristabilisce con l'autostart (Launcher.EnsureConnected, 5 s).
/// </summary>
internal sealed class Repl
{
    private readonly RunspaceSession _session;
    private readonly Executor _executor;
    private readonly OrchestratorClient _client;
    private readonly Launcher _launcher;
    private readonly HostLog _log;

    // Token del turno slash in corso: l'handler di Ctrl+C (altro thread) lo cancella.
    private volatile CancellationTokenSource? _turnCts;

    public Repl(RunspaceSession session, Executor executor, OrchestratorClient client, Launcher launcher, HostLog log)
    {
        _session = session;
        _executor = executor;
        _client = client;
        _launcher = launcher;
        _log = log;
    }

    public int Run()
    {
        // VT processing incondizionata: l'OSC 9001 va interpretata, non stampata come testo.
        ConsoleModes.TryEnableVirtualTerminalProcessing();

        // Lezione dello spike: con stdin rediretto PSConsoleHostReadLine non dà mai EOF → Console.ReadLine.
        bool usePsReadLine = !Console.IsInputRedirected && _session.PsReadLineAvailable;

        ConsoleCancelEventHandler onCancel = (_, e) =>
        {
            e.Cancel = true;                 // non terminare il processo
            _executor.StopCurrent();         // ferma la pipeline (utente o ExecInShell)
            _turnCts?.Cancel();              // sveglia SlashTurn in attesa → CancelCommand
        };
        Console.CancelKeyPress += onCancel;

        try
        {
            Banner(usePsReadLine);
            LoadProfiles();
            ConnectAtStartup();

            while (!_session.Host.ShouldExit)
            {
                Console.Write(_session.EvaluatePrompt());
                string? line = _session.ReadLine(usePsReadLine);
                if (line is null)
                {
                    Console.WriteLine();
                    Console.WriteLine("[LARE] EOF su stdin, uscita.");
                    break;
                }

                if (SlashLine.IsSlash(line))
                {
                    RunSlash(SlashLine.Normalize(line));
                    continue;
                }

                if (string.IsNullOrWhiteSpace(line))
                {
                    continue;
                }

                _executor.RunInteractive(line);
            }
        }
        finally
        {
            Console.CancelKeyPress -= onCancel;
        }

        return _session.Host.ExitCode;
    }

    private void Banner(bool usePsReadLine)
    {
        Console.WriteLine("Lare Terminal " + HostInfo.Version + " — sessione " + _client.SessionId
                          + (usePsReadLine ? string.Empty : "  (PSReadLine non disponibile: editing di riga base)"));
    }

    private void LoadProfiles()
    {
        string docs = Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments);
        ProfileLoader.ProfilePaths paths = ProfileLoader.Compute(docs, _session.PwshDir, AppContext.BaseDirectory);
        ProfileLoader.SetDollarProfile(_session.Runspace, paths);
        IReadOnlyList<string> loaded = ProfileLoader.Load(_session.Runspace, _session.Host, paths, File.Exists);
        _log.Info("profili caricati: " + (loaded.Count == 0 ? "nessuno" : string.Join(", ", loaded)));
    }

    private void ConnectAtStartup()
    {
        if (EnsureConnected())
        {
            Console.WriteLine("orchestratore: connesso");
            _launcher.EnsureUi();
        }
        else
        {
            Console.WriteLine("orchestratore: NON connesso — la shell funziona, i comandi /… no (ritento al prossimo /…)");
        }
    }

    /// <summary>true se connessi (già o dopo autostart+retry).</summary>
    private bool EnsureConnected()
    {
        if (_client.IsConnected)
        {
            return true;
        }

        return _launcher.EnsureConnected(
            () => _client.Connect(_session.CurrentDirectory, TimeSpan.FromSeconds(3)),
            Launcher.ConnectWindow,
            Launcher.RetryInterval);
    }

    private void RunSlash(string input)
    {
        // Segnale all'emulatore (§4.7), prima di qualunque altra cosa: costa nulla e non dipende dal WS.
        Console.Out.Write(Osc.Intercept(input));
        Console.Out.Flush();

        if (!EnsureConnected())
        {
            Console.WriteLine("orchestratore non raggiungibile: comando ignorato");
            return;
        }

        _launcher.EnsureUi();   // self-heal (ruling 8): la finestra di output vive in ui.exe

        using var cts = new CancellationTokenSource();
        _turnCts = cts;
        try
        {
            var turn = new SlashTurn(_client, new ConsoleGate(), _executor, Console.Out, _log);
            TurnResult result = turn.Run(input, _session.CurrentDirectory, cts.Token);
            if (result == TurnResult.Disconnected)
            {
                _log.Warn("turno interrotto per disconnessione; riconnessione al prossimo /…");
            }
        }
        finally
        {
            _turnCts = null;
        }
    }
}
```

- [ ] **Step 5: Implementa `SelfTest.cs` e il `Program.cs` definitivo**

`src/LareShell/SelfTest.cs`:

```csharp
using System.Management.Automation;
using LareShell.Config;
using LareShell.Host;
using LareShell.Shell;

namespace LareShell;

/// <summary>
/// <c>--selftest</c>: controlli non interattivi (nessuna tastiera, nessun orchestratore) con
/// exit code 0/1, per verificare un deploy (Test Run\shell\lare-shell.exe --selftest) o in CI.
/// Stampa una riga [OK]/[FAIL] per controllo, come lo spike.
/// </summary>
internal static class SelfTest
{
    public static int Run(string configDir, StartupConfig cfg)
    {
        bool allOk = true;
        Console.WriteLine("=== lare-shell " + HostInfo.Version + " --selftest ===");
        allOk &= Check("Cartella di configurazione: " + configDir, Directory.Exists(configDir));
        allOk &= Check("startup.json letto (ws_port " + cfg.WsPort + ")", cfg.Warnings.Count == 0, string.Join("; ", cfg.Warnings));
        allOk &= Check("powershell.config.json accanto all'exe", File.Exists(Path.Combine(AppContext.BaseDirectory, "powershell.config.json")));

        string? pwsh = PwshLocator.FindInstallDir();
        allOk &= Check("pwsh trovato", pwsh is not null, pwsh);

        try
        {
            using RunspaceSession s = RunspaceSession.Open(new LareHost(), HostLog.Null);
            allOk &= Check("Runspace aperta", true);
            allOk &= Check("PSReadLine (PSConsoleHostReadLine)", s.PsReadLineAvailable);
            using var ps = PowerShell.Create();
            ps.Runspace = s.Runspace;
            string policy = ps.AddScript("(Get-ExecutionPolicy -Scope LocalMachine).ToString()").Invoke().Single().BaseObject.ToString()!;
            allOk &= Check("Execution policy LocalMachine = RemoteSigned", policy == "RemoteSigned", policy);
        }
        catch (Exception ex)
        {
            allOk &= Check("Runspace aperta", false, ex.Message);
        }

        Console.WriteLine();
        Console.WriteLine(allOk ? "TUTTI I CONTROLLI SONO PASSATI." : "ALCUNI CONTROLLI SONO FALLITI.");
        return allOk ? 0 : 1;
    }

    private static bool Check(string description, bool ok, string? detail = null)
    {
        Console.WriteLine((ok ? "[OK]   " : "[FAIL] ") + description + (string.IsNullOrEmpty(detail) ? string.Empty : " (" + detail + ")"));
        return ok;
    }
}
```

`src/LareShell/Program.cs`:

```csharp
using LareShell.Config;
using LareShell.Host;
using LareShell.Protocol;
using LareShell.Shell;

namespace LareShell;

/// <summary>
/// Entry point: composizione degli oggetti (config → log → runspace → client → launcher → REPL).
/// Nessuna logica qui: solo "chi dipende da chi", così ogni pezzo resta testabile da solo.
/// </summary>
internal static class Program
{
    private static int Main(string[] args)
    {
        try { Console.OutputEncoding = System.Text.Encoding.UTF8; } catch { /* stdout rediretto */ }

        CliArgs cli = CliArgs.Parse(args);
        string configDir = ConfigDir.Resolve(cli.ConfigDir, AppContext.BaseDirectory);
        try { Directory.CreateDirectory(configDir); } catch { /* §9: "creata al primo avvio" — se non si può, i passi dopo lo diranno */ }

        HostLog log = HostLog.Open(configDir);
        StartupConfig cfg = StartupConfig.Load(configDir);
        foreach (string w in cfg.Warnings)
        {
            Console.WriteLine("[LARE] " + w);
            log.Warn(w);
        }

        if (cli.SelfTest)
        {
            return SelfTest.Run(configDir, cfg);
        }

        // Id di sessione: da ui.exe (--session, modalità A) o generato (modalità B). Sempre
        // presente: senza, i log dell'orchestratore non correlano la connessione.
        string sessionId = string.IsNullOrWhiteSpace(cli.SessionId) ? Guid.NewGuid().ToString("N")[..8] : cli.SessionId;
        log.Info("avvio lare-shell " + HostInfo.Version + " sessione " + sessionId + " config " + configDir);

        try
        {
            var host = new LareHost();
            using RunspaceSession session = RunspaceSession.Open(host, log);
            var executor = new Executor(session);
            using var client = new OrchestratorClient(
                new Uri("ws://127.0.0.1:" + cfg.WsPort + "/"),
                () => TokenFile.Read(configDir),
                sessionId,
                HostInfo.Version,
                log);
            var launcher = new Launcher(ConfigDir.DeployRoot(configDir), configDir, cfg, new ProcessStarter(), log, Console.Out);

            return new Repl(session, executor, client, launcher, log).Run();
        }
        catch (Exception ex)
        {
            // Ultima rete: un errore non previsto all'avvio (es. runspace che non si apre) deve
            // lasciare una riga leggibile, non uno stack trace in una scheda che si chiude.
            Console.Error.WriteLine("[LARE] errore fatale: " + ex.GetType().Name + ": " + ex.Message);
            log.Warn("errore fatale: " + ex);
            return 1;
        }
    }
}
```

- [ ] **Step 6: Verifica: suite intera, selftest, avvio manuale**

```powershell
dotnet test shell/lare-shell/LareShell.sln
dotnet run --project shell/lare-shell/src/LareShell -- --config-dir "Test Run\Configuration" --selftest
```

Atteso: suite tutta verde; selftest `TUTTI I CONTROLLI SONO PASSATI.`, exit 0. Poi, con
l'orchestratore e `ui.exe` avviati come in RUN-LOCAL (`cargo run -p orchestrator -- --config-dir
"Test Run\Configuration" --console-log` e `cargo run -p ui -- --config-dir "Test Run\Configuration"`),
in un terminale **Windows Terminal/pwsh vero** (non nel terminale di Claude Code, che ha stdin
rediretto):

```powershell
dotnet run --project shell/lare-shell/src/LareShell -- --config-dir "Test Run\Configuration"
```

e verifica a mano: prompt di pwsh, `Get-Date` funziona, `/ping` apre la finestra "Lare — /ping" su
`ui.exe` e stampa `→ finestra "/ping" aperta`, `/nonesiste` è muto, `exit` chiude. Riporta cosa hai
visto nel report (questo è il primo e2e reale del canale shell con una host vera). Se non puoi
aprire un terminale interattivo, dillo esplicitamente nel report: il controller farà l'e2e.

- [ ] **Step 7: Commit**

```powershell
git add shell/lare-shell
git commit -m "feat(lare-shell): REPL completo (PSReadLine, profili, slash → SlashTurn, Ctrl+C, riconnessione), OSC 9001, --selftest"
```

### Task 8: Profilo Windows Terminal (`install`/`uninstall-wt-profile.ps1`) e `deploy_test_run.ps1` con `dotnet publish`

**Modello: Haiku** (script piccoli, contenuto completo qui sotto).

**Files:**
- Create: `Test Run/install-wt-profile.ps1`, `Test Run/uninstall-wt-profile.ps1`
- Modify: `deploy_test_run.ps1` (parametro `-SkipShell`, blocco `dotnet publish`)

Riferimento: `spikes/lare-shell-host/install-wt-profile.ps1` e `uninstall-wt-profile.ps1` (stessa
tecnica: fragment JSON di Windows Terminal). Differenze: nome profilo **"Lare Terminal"**, exe
`Test Run\shell\lare-shell.exe`, file `lare-terminal.json`, commandline **tra virgolette** (il
percorso del repo contiene spazi), niente icona.

- [ ] **Step 1: Script del profilo WT**

`Test Run/install-wt-profile.ps1`:

```powershell
<#
.SYNOPSIS  Installa il profilo "Lare Terminal" in Windows Terminal (modalità B, spec §2.3).
.DESCRIPTION
  Scrive un "fragment" JSON in %LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Lare\
  (è la cartella in cui Windows Terminal cerca i profili aggiunti da app terze: leggere
  LOCALAPPDATA qui è integrazione con WT, non configurazione Lare — spec §6.1). Il profilo avvia
  shell\lare-shell.exe nudo: la host risolve la Configuration come <exe>\..\Configuration\ e, se
  servono, avvia orchestratore e ui.exe da sola (§6.4). Riavvia Windows Terminal dopo l'installazione.
#>
param(
    [string]$ShellExe = (Join-Path $PSScriptRoot "shell\lare-shell.exe")
)
$ErrorActionPreference = "Stop"
if (-not (Test-Path $ShellExe)) {
    throw "Manca $ShellExe - esegui prima deploy_test_run.ps1 (fa il dotnet publish in Test Run\shell\)."
}
$fragmentDir = Join-Path $env:LOCALAPPDATA "Microsoft\Windows Terminal\Fragments\Lare"
New-Item -ItemType Directory -Force $fragmentDir | Out-Null
$fragmentPath = Join-Path $fragmentDir "lare-terminal.json"
$fragment = [ordered]@{
    profiles = @(
        [ordered]@{
            name              = "Lare Terminal"
            # Virgolette obbligatorie: il percorso contiene spazi ("Test Run").
            commandline       = "`"$ShellExe`""
            startingDirectory = "%USERPROFILE%"
        }
    )
}
$fragment | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 $fragmentPath
Write-Host "Profilo 'Lare Terminal' installato in $fragmentPath"
Write-Host "Riavvia Windows Terminal e aprilo dal menu a tendina delle schede."
```

`Test Run/uninstall-wt-profile.ps1`:

```powershell
<#
.SYNOPSIS  Rimuove il profilo "Lare Terminal" da Windows Terminal (il fragment scritto da install-wt-profile.ps1).
#>
$ErrorActionPreference = "Stop"
$fragmentPath = Join-Path $env:LOCALAPPDATA "Microsoft\Windows Terminal\Fragments\Lare\lare-terminal.json"
if (Test-Path $fragmentPath) {
    Remove-Item $fragmentPath -Force
    Write-Host "Profilo 'Lare Terminal' rimosso ($fragmentPath). Riavvia Windows Terminal."
} else {
    Write-Host "Nessun profilo da rimuovere ($fragmentPath non esiste)."
}
```

- [ ] **Step 2: `deploy_test_run.ps1` pubblica la host**

Aggiungi il parametro e il blocco (dopo il ciclo che copia gli exe Rust, prima di `robocopy`):

```powershell
param(
    [ValidateSet("debug", "release")] [string]$BuildConfig = "debug",
    [switch]$IncludePlugins,
    # Salta il dotnet publish della host C# (lento, ~1 min): utile quando si ricompila solo il Rust.
    [switch]$SkipShell
)
```

```powershell
if (-not $SkipShell) {
    # Host C# (spec §7): publish framework-dependent per win-x64 in Test Run\shell\ — serve il
    # runtime .NET 10 sulla macchina di destinazione (DEPLOY.md), in cambio ~100 MB invece di ~200.
    # Sempre Release: la host non ha una build "debug" utile nel deploy.
    $proj = Join-Path $Repo "shell\lare-shell\src\LareShell\LareShell.csproj"
    $shellOut = Join-Path $Dest "shell"
    & dotnet publish $proj -c Release -r win-x64 --self-contained false -o $shellOut --nologo -v quiet
    if ($LASTEXITCODE -ne 0) { throw "dotnet publish della host fallita (exit $LASTEXITCODE)" }
    Write-Host "pubblicata lare-shell in shell\"
}
```

Aggiorna anche il commento `.DESCRIPTION` in testa: aggiungi la riga
`shell\lare-shell (dotnet publish, Release, win-x64, framework-dependent) -> Test Run\shell\ (salta con -SkipShell);`.

- [ ] **Step 3: Verifica**

```powershell
.\deploy_test_run.ps1
Get-ChildItem "Test Run\shell\lare-shell.exe", "Test Run\shell\powershell.config.json"
& "Test Run\shell\lare-shell.exe" --selftest
(Get-ChildItem "Test Run\shell" -Recurse | Measure-Object Length -Sum).Sum / 1MB
.\Test Run\install-wt-profile.ps1
Get-Content "$env:LOCALAPPDATA\Microsoft\Windows Terminal\Fragments\Lare\lare-terminal.json"
git status --short
```

Atteso: i due file esistono; selftest tutto `[OK]` con `Cartella di configurazione: …\Test Run\Configuration`
(risolta da `<exe>\..\Configuration`, senza `--config-dir`); dimensione della cartella riportata nel
report (per DEPLOY.md); fragment con `"commandline": "\"…\\Test Run\\shell\\lare-shell.exe\""`;
`git status` NON mostra nulla sotto `Test Run/shell/` (gitignore del Task 0).

- [ ] **Step 4: Commit**

```powershell
git add deploy_test_run.ps1 "Test Run/install-wt-profile.ps1" "Test Run/uninstall-wt-profile.ps1"
git commit -m "feat(deploy): dotnet publish della host in Test Run\shell e profilo Windows Terminal 'Lare Terminal'"
```

---

### Task 9: Documentazione, ADR-019, release `lare-shell` 2.0.0 ed e2e in Windows Terminal

**Files:**
- Create: `shell/lare-shell/CHANGELOG.md`, `shell/lare-shell/IMPLEMENTATION.md`
- Modify: `Docs/i18n/ita/06-decisions.md` (ADR-019), `Docs/i18n/ita/DEPLOY.md` (prerequisiti, layout `shell\`, passi),
  `Docs/i18n/ita/RUN-LOCAL.md` (build/test .NET, modalità B, avvio da sorgente), `Docs/i18n/ita/TESTING-e2e.md`
  (Parte 6), `Docs/i18n/ita/KNOWN-ISSUES.md`, `Docs/i18n/ita/HANDOFF.md` (versioni, FATTO, DA FARE, debiti),
  `Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` (emendamenti §4.4, §6.4, §9),
  `CLAUDE.md` (comandi .NET, `shell/lare-shell/`)

**E2E dal vivo (prima del commit di release, in Windows Terminal — la fa il controller se
l'implementer non ha un terminale interattivo):** con `deploy_test_run.ps1` fatto e NESSUN
orchestratore/ui.exe in esecuzione (`Get-Process orchestrator, ui -ErrorAction SilentlyContinue | Stop-Process`),
apri la scheda "Lare Terminal" e verifica, annotando l'esito di ognuno in TESTING-e2e Parte 6:

1. banner + "orchestratore: NON connesso … avvio …\orchestrator.exe" + "orchestratore avviato e connesso"; `ui.exe` compare (autostart §6.4);
2. prompt e profilo dell'utente come in pwsh (alias, oh-my-posh se c'è); `Get-Date`, `dir`, `cd ..` funzionano;
3. `/ping` → finestra "Lare — /ping" su `ui.exe` con le righe orchestrator/plugin/ui/lare-shell 2.0.0 + `→ finestra "/ping" aperta` nel terminale;
4. `/help`, `/config`, `/library` → finestre giuste; `/nonesiste` → muto; `/ai x` senza virgolette → riga `errore: sintassi: /ai "testo" (virgolette obbligatorie)`;
5. `/ai "elenca i 3 file più grandi in questa cartella"` → `[Y/n]` → Invio → comando eseguito NEL terminale (output visibile) → finestra Markdown col risultato;
6. `/ai "vai nella cartella Documents"` → `[Y/n]` → il prompt dopo mostra `Documents` (cwd persiste, D17);
7. `/ai "cancella tutti i file temporanei"` → `n` al gate → nessun comando eseguito, il turno finisce;
8. Ctrl+C in attesa del turno (`/ai "conta fino a un milione lentamente"`, subito Ctrl+C) → "annullato (Ctrl+C)", prompt; Ctrl+C durante un `ExecInShell` lungo (`/ai "esegui Start-Sleep 60"`, Y, Ctrl+C) → "comando interrotto (Ctrl+C): turno annullato";
9. `/ai "apri python in modo interattivo"` (o `/ai "avvia python"`) con `interactive` → REPL di python utilizzabile, `exit()` torna al prompt;
10. chiudi `ui.exe` → `/help` la riavvia (self-heal, ruling 8); uccidi l'orchestratore (`Stop-Process -Name orchestrator`) → `/ping` → "orchestratore non raggiungibile … avvio …" → riconnesso e finestra aperta;
11. `exit` → la scheda si chiude; `Get-Process lare-shell -ErrorAction SilentlyContinue` → nulla, ma
    `Get-Process orchestrator, ui` → ANCORA vivi (è il senso di `UseShellExecute=true`: figli staccati);
12. copia `Test Run\` in `%TEMP%\LareCopia\`, esegui `LareCopia\shell\lare-shell.exe --selftest` → `[OK]` con la Configuration di LareCopia (percorsi relativi, §6.3).

- [ ] **Step 1: `CHANGELOG.md` e `IMPLEMENTATION.md` di `shell/lare-shell/`**

`CHANGELOG.md` (semver, da 2.0.0):

```markdown
# Changelog — lare-shell

## 2.0.0 — 2026-09-06 (piano 2b)

Prima versione della host custom del motore PowerShell (ADR-015), verificabile in modalità B
(profilo Windows Terminal "Lare Terminal").

- Configurazione: `--config-dir` o `<exe>\..\Configuration\`, `startup.json` (ws_port, autostart), `token`,
  log su file `logs\lare-shell.log` (D6: nessuna variabile d'ambiente).
- Protocollo 2.1 (canale shell): `hello{role:"shell"}`, `command`, `tool_confirm_response`, `exec_result`,
  `cancel_command`; ricezione su thread proprio → `Channel`, consumo sul thread del REPL.
- Runspace ospitata: PSReadLine dai moduli di pwsh (anteposti al PSModulePath del processo),
  execution policy LocalMachine da `powershell.config.json` accanto all'exe (ADR-019), profili
  `profile.ps1` → `Microsoft.PowerShell_profile.ps1` → `LareShell_profile.ps1`, `$PROFILE` con 4 NoteProperty.
- Turno slash: gate `[Y/n]` a lettura tasto, `ExecInShell` con `capture` true (cmdlet di passaggio,
  output catturato dai `Write*` della host, cap 200 KB testa+coda) / false (console attaccata),
  `exit_code` da `$?`/`$LASTEXITCODE`, cwd per sessione, Ctrl+C = stop + cancel.
- Autostart di `orchestrator.exe` (retry 5 s) e `ui.exe` (§6.4), processi senza console ereditata.
- OSC 9001 `intercept` (ESC da `(char)0x1B`, test di scansione dei sorgenti).
- `--selftest`; xUnit: ≈100 test, con server WS finto in-process.
```

`IMPLEMENTATION.md`: struttura in tre strati (Config/Protocol/Shell+Host) con una riga per classe,
l'invariante "un solo thread tocca runspace e console", il flusso di un turno (sequenza §4.2 con i
nomi delle classi), come si testa (`dotnet test shell/lare-shell/LareShell.sln`; collection
`runspace`; server finto `FakeOrchestrator` su `TcpListener` e perché non `HttpListener`), i ruling
1–9 del piano, i debiti (profili AllUsers non caricati; log senza rotazione; `$?` dopo un turno
slash; `ToolConfirmRequest` senza `turn_id`; `PromptForCredential` non implementato; nessun test
automatico di `Repl`/`ConsoleGate` — coperti dall'e2e Parte 6).

- [ ] **Step 2: ADR-019 in `06-decisions.md`** (appendi dopo ADR-018)

```markdown
## ADR-019 — Host `lare-shell`: policy da file, profili di pwsh, cattura via host UI, un thread per runspace e console (2026-09-06)

**Contesto.** Lo spike forzava l'execution policy in-process, non caricava il profilo di pwsh, non
parlava col WS e non catturava l'output. Il prodotto deve comportarsi "come pwsh" (§4.4) e parlare
il canale shell del piano 2a rispettando i contratti (a)–(d).

**Decisione.** (1) L'execution policy LocalMachine viene da un `powershell.config.json` spedito
accanto all'exe (`$PSHOME` di una host = la sua cartella): `Set-ExecutionPolicy` la cambia come in
pwsh. (2) PSReadLine viene dai moduli di pwsh, anteposti al `PSModulePath` del processo (pwsh 7.6+
è prerequisito). (3) Profili CurrentUser in ordine `profile.ps1`, `Microsoft.PowerShell_profile.ps1`,
`LareShell_profile.ps1`; `$PROFILE` come pwsh; AllUsers non caricati. (4) Cattura dell'output
(`capture:true`) registrando i `Write*` della `PSHostUserInterface`, con `ForEach-Object { $_ }`
fra script e `Out-Default` perché anche i nativi passino dalla pipe; `capture:false` = pipeline pura.
(5) Un solo thread (REPL) possiede runspace e console; il socket accoda in un `Channel`; nessun
`async` nel REPL. (6) Riconnessione on demand con autostart (5 s), processi figli con
`UseShellExecute=true` (nessuna console ereditata). (7) `exit_code` = `$?` catturato in coda allo
stesso script (`$global:__lare_ok = $?`) e `$LASTEXITCODE` azzerato prima del comando.

**Conseguenze.** La host è un pwsh "vero" per l'utente (PSReadLine, profilo, prompt) più i `/…`;
i test girano contro un server WS finto su `TcpListener` (mai `HttpListener`); debiti in HANDOFF.
```

- [ ] **Step 3: DEPLOY, RUN-LOCAL, TESTING-e2e, KNOWN-ISSUES, spec, CLAUDE.md**

- `DEPLOY.md` §Prerequisiti: aggiungi **pwsh 7.6+** (PSReadLine e profili vengono da lì) e **.NET 10
  runtime** (host framework-dependent; dimensione di `shell\` misurata nel Task 8); §Layout: riga
  `shell\` (lare-shell.exe + DLL del motore + `powershell.config.json`) e `install-wt-profile.ps1`;
  §Passi: `deploy_test_run.ps1` (publish incluso), `install-wt-profile.ps1`, `lare-shell.exe --selftest`.
- `RUN-LOCAL.md`: sezione "Host C# (`shell/lare-shell/`)": `dotnet build shell/lare-shell/LareShell.sln`,
  `dotnet test shell/lare-shell/LareShell.sln` (filtri `--filter "FullyQualifiedName~Executor"`),
  avvio da sorgente `dotnet run --project shell/lare-shell/src/LareShell -- --config-dir "Test Run\Configuration"`
  (da sorgente `..\Configuration` non esiste: `--config-dir` sempre), modalità B (`deploy_test_run.ps1`
  + `install-wt-profile.ps1`), gotcha: il terminale di Claude Code ha stdin rediretto → niente
  PSReadLine, usare Windows Terminal per l'e2e; `taskkill //F //IM lare-shell.exe` se `dotnet build`
  dà "accesso negato".
- `TESTING-e2e.md`: **Parte 6 — Modalità B in Windows Terminal (host `lare-shell`)** con i 12 punti
  dell'e2e sopra e l'esito reale di ciascuno (data, OK/KO, note).
- `KNOWN-ISSUES.md`: profili AllUsers non caricati; log della host senza rotazione; dopo un turno
  slash `$?` nel prompt è sempre vero (ruling 3); `ui.exe` avviata da `lare-shell` con
  `UseShellExecute` — se la finestra non compare in primo piano è il focus di Windows, non un bug;
  `ToolConfirmRequest` non porta il `turn_id` (attribuita al turno corrente).
- Spec: §4.4 "Profilo" → "Confermato (piano 2b): … in quest'ordine"; §4.4 "Execution policy" → "Risolta
  (piano 2b, ADR-019): `powershell.config.json` accanto all'exe"; §6.4 "Processi staccati" → "Nella host
  C#: `UseShellExecute=true` + finestra nascosta (equivalente pratico di DETACHED_PROCESS, ruling 4 del
  piano 2b)"; §9 riga "WS cade durante un turno" → "riconnessione al prossimo `/…` (on demand, con
  autostart)"; §13: spunta le voci execution policy, profili, codepage (`Console.OutputEncoding = UTF8`
  è anche l'encoding con cui PowerShell decodifica lo stdout dei nativi), publish, protocollo.
- `CLAUDE.md`: nel blocco Comandi aggiungi `dotnet test shell/lare-shell/LareShell.sln` e
  `.\deploy_test_run.ps1` (publish incluso, `-SkipShell` per saltarlo); in Architettura la riga
  "Host C# in `shell/lare-shell/` (src + tests xUnit)"; nel Gotcha `taskkill` aggiungi `//IM lare-shell.exe`.

- [ ] **Step 4: HANDOFF**

- Versioni: aggiungi `lare-shell` **2.0.0**.
- FATTO: "Piano 2b — host C# `lare-shell` (modalità B): …" con i commit del branch.
- DA FARE: togli il piano 2b; nel piano 3 aggiungi "modalità A: `ui.exe` lancia `lare-shell.exe
  --config-dir … --session <id>` in ConPTY; consumare l'OSC 9001 `intercept`; `ActivityIndicator`";
  chore fmt invariata.
- "Debiti / decisioni del piano 2b": i ruling 1–9 in una riga ciascuno + i debiti di KNOWN-ISSUES +
  "test di `Repl`/`ConsoleGate` solo e2e".

- [ ] **Step 5: Verifica finale e commit di release**

```powershell
dotnet test shell/lare-shell/LareShell.sln
cargo test                       # il Rust non è toccato: deve restare 1475 verdi
git status --short               # solo docs + shell/lare-shell/*.md + CLAUDE.md
```

Poi (hook `commit-msg`: HANDOFF nello stesso commit della release):

```powershell
git add shell/lare-shell/CHANGELOG.md shell/lare-shell/IMPLEMENTATION.md Docs CLAUDE.md
git commit -m "release: piano 2b completato — host C# lare-shell 2.0.0 (modalità B in Windows Terminal), ADR-019, e2e Parte 6"
```
