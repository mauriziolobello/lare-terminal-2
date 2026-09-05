//! # mcp-nmap — stdio entry point
//!
//! Thin MCP glue layer, mirroring `crates/mcp-server/src/main.rs`. All
//! business logic lives in `mcp_nmap::scan`/`report`/`markdown`/`elevate`.
//!
//! Both tools return a bare `String` (the JSON-serialized `ScanOutcome`) —
//! same convention as `mcp-server`'s tools: success/error are both encoded
//! as DATA inside the JSON (`is_error: bool`), never as an MCP-protocol-level
//! error. The orchestrator's `NmapToolClient` (companion integration plan)
//! deserializes this JSON directly.

use mcp_nmap::network_info;
use mcp_nmap::scan::{self, NmapProcess, ScanOutcome};
use rmcp::{
    handler::server::wrapper::Parameters, schemars, tool, tool_router, transport::stdio, ServiceExt,
};
use serde::Deserialize;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use tracing_subscriber::{fmt, EnvFilter};

/// Real `NmapProcess`: spawns `nmap` as a genuine child process. Only used
/// here (never in tests — those use a fake, see `scan.rs`'s test module).
struct RealNmapProcess;

impl NmapProcess for RealNmapProcess {
    fn run(&self, args: &[&str], _xml_path: &Path) -> Result<ExitStatus, String> {
        #[cfg(windows)]
        let exe = "nmap.exe";
        #[cfg(not(windows))]
        let exe = "nmap";
        // stdout/stderr are deliberately discarded (Stdio::null()), NOT
        // inherited (std::process::Command's default): nmap always writes
        // its human-readable scan report to its own stdout/stderr in
        // addition to the -oX XML file this crate actually reads. Without
        // this, that inherits *this process's* stdout — which is the same
        // stream rmcp's stdio transport uses for the MCP JSON-RPC wire —
        // and nmap's plain-text banner/port-table would interleave with (and
        // corrupt) every JSON-RPC message the orchestrator's client reads.
        // Found live during Task 5's manual smoke test (a real scan against
        // 127.0.0.1) before this fix: nmap's console output appeared
        // verbatim on stdout, and the tool's own JSON-RPC response never
        // arrived cleanly on that same stream.
        std::process::Command::new(exe)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("spawn di nmap fallito: {e}"))
    }
}

/// Input parameters for the `nmap_quick_scan` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NmapQuickScanParams {
    /// IP, range CIDR, o hostname da scansionare.
    target: String,
}

/// Input parameters for the `nmap_os_detect` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NmapOsDetectParams {
    /// IP, range CIDR, o hostname da scansionare. Richiede privilegi elevati.
    target: String,
}

/// Input parameters for the `nmap_version_scan` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NmapVersionScanParams {
    /// IP, range CIDR, o hostname da scansionare.
    target: String,
}

/// Input parameters for the `nmap_host_discovery` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NmapHostDiscoveryParams {
    /// IP, range CIDR, o hostname/rete su cui scoprire gli host attivi.
    target: String,
}

/// Input parameters for the `nmap_vuln_scan` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NmapVulnScanParams {
    /// IP, range CIDR, o hostname da sottoporre a scansione vulnerabilità.
    target: String,
}

/// Input parameters for the `traceroute` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TracerouteParams {
    /// IP o hostname verso cui tracciare il percorso di rete.
    target: String,
}

#[derive(Clone)]
struct NmapServer;

#[tool_router(server_handler)]
impl NmapServer {
    /// Esegue una scansione TCP connect (`-sT`, non elevata) e ritorna
    /// `{ summary, report_markdown, is_error }` come stringa JSON.
    #[tool(
        description = "Scansione nmap rapida (TCP connect, -sT, nessun privilegio elevato richiesto). Ritorna { summary, report_markdown, is_error }."
    )]
    async fn nmap_quick_scan(
        &self,
        Parameters(NmapQuickScanParams { target }): Parameters<NmapQuickScanParams>,
    ) -> String {
        let outcome: ScanOutcome =
            scan::run_quick_scan(&target, &RealNmapProcess, scan::nmap_on_path());
        serde_json::to_string(&outcome).unwrap()
    }

    /// Esegue un rilevamento OS (`-O`, ELEVATO — UAC) e ritorna
    /// `{ summary, report_markdown, is_error }` come stringa JSON.
    #[tool(
        description = "Rilevamento OS nmap (-O). RICHIEDE PRIVILEGI ELEVATI: mostra un prompt UAC di Windows. Ritorna { summary, report_markdown, is_error }."
    )]
    async fn nmap_os_detect(
        &self,
        Parameters(NmapOsDetectParams { target }): Parameters<NmapOsDetectParams>,
    ) -> String {
        let outcome: ScanOutcome = scan::run_os_detect(&target, scan::nmap_on_path());
        serde_json::to_string(&outcome).unwrap()
    }

    /// Scansione nmap con rilevamento versioni servizi (-sV). Non elevato.
    /// Ritorna `{ summary, report_markdown, is_error }` come stringa JSON.
    #[tool(
        description = "Scansione nmap con rilevamento versioni dei servizi (-sV, nessun privilegio elevato richiesto). Più lenta di nmap_quick_scan ma identifica software/versioni in ascolto sulle porte aperte."
    )]
    async fn nmap_version_scan(
        &self,
        Parameters(NmapVersionScanParams { target }): Parameters<NmapVersionScanParams>,
    ) -> String {
        let outcome = scan::run_version_scan(&target, &RealNmapProcess, scan::nmap_on_path());
        serde_json::to_string(&outcome).unwrap()
    }

    /// Host discovery nmap (-sn): quali host sono attivi, nessuna scansione
    /// porte. Non elevato. Ritorna `{ summary, report_markdown, is_error }`.
    #[tool(
        description = "Scoperta host attivi su una rete/range (-sn, nessuna scansione porte, nessun privilegio elevato). Più rapida di uno scan completo quando serve solo sapere quali dispositivi sono accesi."
    )]
    async fn nmap_host_discovery(
        &self,
        Parameters(NmapHostDiscoveryParams { target }): Parameters<NmapHostDiscoveryParams>,
    ) -> String {
        let outcome = scan::run_host_discovery(&target, &RealNmapProcess, scan::nmap_on_path());
        serde_json::to_string(&outcome).unwrap()
    }

    /// Scansione vulnerabilità nmap (--script vuln, categoria NSE curata da
    /// nmap stesso). Non elevato. Ritorna `{ summary, report_markdown, is_error }`.
    #[tool(
        description = "Scansione vulnerabilità nota tramite gli script NSE 'vuln' integrati in nmap (--script vuln). Nessun privilegio elevato richiesto. Più lenta degli altri scan: verifica vulnerabilità note sui servizi rilevati."
    )]
    async fn nmap_vuln_scan(
        &self,
        Parameters(NmapVulnScanParams { target }): Parameters<NmapVulnScanParams>,
    ) -> String {
        let outcome = scan::run_vuln_scan(&target, &RealNmapProcess, scan::nmap_on_path());
        serde_json::to_string(&outcome).unwrap()
    }

    /// Informazioni di rete della macchina locale (ipconfig /all, arp -a,
    /// route print, netstat -rn) e ritorna `{ output, is_error }` come
    /// stringa JSON. Nessun parametro.
    #[tool(
        description = "Informazioni di rete della macchina locale (ipconfig /all, arp -a, route print, netstat -rn). Nessun parametro. Usa questo PRIMA di uno scan per determinare il proprio IP/subnet quando l'utente non lo specifica. Ritorna { output, is_error }."
    )]
    async fn local_network_info(&self) -> String {
        let outcome = network_info::local_network_info();
        serde_json::to_string(&outcome).unwrap()
    }

    /// Traccia il percorso di rete (`tracert`) verso `target` e ritorna
    /// `{ output, is_error }` come stringa JSON.
    #[tool(
        description = "Traccia il percorso di rete (tracert) verso un host. Ritorna { output, is_error }."
    )]
    async fn traceroute(
        &self,
        Parameters(TracerouteParams { target }): Parameters<TracerouteParams>,
    ) -> String {
        let outcome = network_info::traceroute(&target);
        serde_json::to_string(&outcome).unwrap()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // All logs go to stderr — stdout is the MCP JSON-RPC wire.
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Lare Terminal mcp-nmap v0.8.2 starting (transport: stdio)");

    let service = NmapServer.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
