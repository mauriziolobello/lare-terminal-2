//! # mcp-nmap
//!
//! Sidecar MCP server exposing seven tools — five structured nmap scans
//! (`nmap_quick_scan`, `nmap_os_detect`, `nmap_version_scan`,
//! `nmap_host_discovery`, `nmap_vuln_scan`) and two built-in network-info
//! commands (`local_network_info`, `traceroute`) — see
//! `Docs/superpowers/specs/2026-07-16-mcp-nmap-design.md`,
//! `Docs/superpowers/plans/2026-07-18-nmap-network-info-tools.md`, and
//! `Docs/superpowers/plans/2026-07-18-nmap-scan-variants.md`.
//!
//! ## Six-module architecture (mirrors `crates/mcp-server`'s SRP split)
//!
//! 1. **`elevate`** — Windows-only `ShellExecuteExW` elevation, isolated so
//!    the rest of the crate stays platform-agnostic (Task 4).
//! 2. **`markdown`** — `ScanReport` → Markdown report + brief LLM summary (Task 2).
//! 3. **`network_info`** — built-in OS network diagnostic commands
//!    (`ipconfig`/`arp`/`route`/`netstat`/`tracert` on Windows), no `nmap`
//!    dependency; returns full output directly to the model, no
//!    `report_markdown`/window/Library split (2026-07-18 plan, Task 1).
//! 4. **`report`** — pure XML parsing + data model (`ScanReport`), zero MCP dep.
//! 5. **`scan`** — process invocation. `run_quick_scan` (unelevated) goes
//!    through a mockable `NmapProcess` trait so tests never spawn a real
//!    `nmap` process (Task 3). `run_os_detect` (`-O`) calls `elevate`
//!    directly instead — elevation itself isn't mockable (Task 4).
//! 6. **`main.rs`** — thin MCP glue: `#[tool_router]`, parameter structs,
//!    calls into the above (Task 5).

pub mod elevate;
pub mod fritzbox;
pub mod markdown;
pub mod network_info;
pub mod report;
pub mod scan;
