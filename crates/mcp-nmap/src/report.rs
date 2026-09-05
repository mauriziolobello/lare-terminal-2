//! # report — pure XML parsing + data model for nmap scan results
//!
//! Zero dependency on MCP/rmcp: this module only knows nmap's `-oX` XML
//! schema and produces a plain data structure. Testable entirely with
//! fixture XML strings, no real `nmap` process involved.

use roxmltree::Document;

/// One NSE script result (`<script id="..." output="...">`) — present
/// whenever nmap ran with `--script ...` (currently only `nmap_vuln_scan`
/// does). Only the `output` attribute is captured (nmap's own flattened
/// text, already entity-decoded by roxmltree) — nested `<table>`/`<elem>`
/// structured children some scripts also emit are deliberately not parsed
/// (out of scope for a correct first fix).
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptResult {
    pub id: String,
    pub output: String,
}

/// One TCP/UDP port entry from a host's `<ports>` block.
#[derive(Debug, Clone, PartialEq)]
pub struct PortReport {
    pub port: u16,
    pub protocol: String,
    /// "open" / "closed" / "filtered" — verbatim from nmap's `<state state="...">`.
    pub state: String,
    /// `<service name="...">`, absent when nmap couldn't identify the service.
    pub service: Option<String>,
    /// Per-port NSE script results (`<port><script ...>`), e.g. from
    /// `--script vuln`. Empty for scans that don't run scripts.
    pub scripts: Vec<ScriptResult>,
}

/// One `<host>` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct HostReport {
    pub address: String,
    /// `<status state="up|down">` — "zero host up" is a legitimate result,
    /// not an error (design spec §7): callers must not treat `up: false` as
    /// a failure.
    pub up: bool,
    pub ports: Vec<PortReport>,
    /// `<osmatch name="...">` entries, ordered as nmap reported them
    /// (most-accurate first). Empty for `nmap_quick_scan` (no `-O`), and
    /// empty for `nmap_os_detect` when nmap made no confident match.
    pub os_matches: Vec<String>,
    /// Host-level NSE script results (`<hostscript><script ...>`), e.g.
    /// `--script vuln`'s SMB checks that aren't tied to one port. Empty for
    /// scans that don't run scripts.
    pub host_scripts: Vec<ScriptResult>,
}

/// A full parsed scan result.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanReport {
    /// The target string as passed to nmap (echoed back for the report header).
    pub target: String,
    /// True if this was an `-O` (`nmap_os_detect`) scan — controls whether the
    /// Markdown/summary formatting (Task 2) mentions OS detection at all.
    pub os_detection: bool,
    pub hosts: Vec<HostReport>,
}

/// XML parsing failed — the nmap XML did not have the expected shape, or was
/// not valid XML at all. Carries nmap's raw parse-failure reason for
/// diagnostics (design spec §7: "aiuta a distinguere nmap è crashato da bug
/// nel parser").
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    MalformedXml(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MalformedXml(reason) => write!(f, "XML nmap malformato: {reason}"),
        }
    }
}

/// Parses every direct `<script id="..." output="...">` child of `parent`
/// into a `ScriptResult`. Used for both `<port>` (per-port scripts) and
/// `<hostscript>` (host-level scripts) — same element shape, different
/// parent. Missing `id`/`output` attributes become empty strings rather
/// than panicking (matches this parser's existing style elsewhere, e.g.
/// `protocol`/`state`).
fn parse_scripts(parent: roxmltree::Node) -> Vec<ScriptResult> {
    parent
        .children()
        .filter(|n| n.has_tag_name("script"))
        .map(|n| ScriptResult {
            id: n.attribute("id").unwrap_or("").to_string(),
            output: n.attribute("output").unwrap_or("").to_string(),
        })
        .collect()
}

/// Parse nmap's `-oX` XML output into a [`ScanReport`].
///
/// `os_detection` is passed by the caller (not read from the XML) because it
/// reflects which TOOL was invoked (`nmap_quick_scan` vs `nmap_os_detect`),
/// not something nmap itself declares in a single dedicated field.
pub fn parse_nmap_xml(xml: &str, os_detection: bool) -> Result<ScanReport, ParseError> {
    // `allow_dtd: true` — real nmap `-oX` output always opens with
    // `<!DOCTYPE nmaprun>` (confirmed against a live scan; see this module's
    // `parses_real_nmap_output_including_doctype_and_stylesheet_pi` test).
    // roxmltree rejects any DTD declaration by default (`ParsingOptions`'s
    // `Default` sets `allow_dtd: false`) as a defense against XML entity-
    // expansion attacks; nmap's DOCTYPE here has no internal/external
    // subset (just the bare declaration), so there is nothing to expand —
    // allowing it is safe. Without this, `parse_nmap_xml` rejected every
    // real nmap scan on every machine as "malformed", a crate-breaking bug
    // that Task 1-4's hand-written fixtures (which never included a
    // DOCTYPE) never exercised.
    let opts = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = Document::parse_with_options(xml, opts)
        .map_err(|e| ParseError::MalformedXml(e.to_string()))?;

    let target = doc
        .root_element()
        .attribute("args")
        .map(|args| {
            // `args` is the full command line nmap was invoked with, e.g.
            // "nmap -sT -oX - 192.168.1.10" — the target is the last word.
            args.split_whitespace().last().unwrap_or("").to_string()
        })
        .unwrap_or_default();

    let mut hosts = Vec::new();
    for host_node in doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("host"))
    {
        let up = host_node
            .children()
            .find(|n| n.has_tag_name("status"))
            .and_then(|n| n.attribute("state"))
            .map(|s| s == "up")
            .unwrap_or(false);

        let address = host_node
            .children()
            .find(|n| n.has_tag_name("address"))
            .and_then(|n| n.attribute("addr"))
            .unwrap_or("")
            .to_string();

        let mut ports = Vec::new();
        if let Some(ports_node) = host_node.children().find(|n| n.has_tag_name("ports")) {
            for port_node in ports_node.children().filter(|n| n.has_tag_name("port")) {
                let port = port_node
                    .attribute("portid")
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(0);
                let protocol = port_node.attribute("protocol").unwrap_or("").to_string();
                let state = port_node
                    .children()
                    .find(|n| n.has_tag_name("state"))
                    .and_then(|n| n.attribute("state"))
                    .unwrap_or("")
                    .to_string();
                let service = port_node
                    .children()
                    .find(|n| n.has_tag_name("service"))
                    .and_then(|n| n.attribute("name"))
                    .map(str::to_string);
                let scripts = parse_scripts(port_node);
                ports.push(PortReport {
                    port,
                    protocol,
                    state,
                    service,
                    scripts,
                });
            }
        }

        let os_matches = host_node
            .children()
            .find(|n| n.has_tag_name("os"))
            .map(|os_node| {
                os_node
                    .children()
                    .filter(|n| n.has_tag_name("osmatch"))
                    .filter_map(|n| n.attribute("name").map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let host_scripts = host_node
            .children()
            .find(|n| n.has_tag_name("hostscript"))
            .map(parse_scripts)
            .unwrap_or_default();

        hosts.push(HostReport {
            address,
            up,
            ports,
            os_matches,
            host_scripts,
        });
    }

    Ok(ScanReport {
        target,
        os_detection,
        hosts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST_UP_OPEN_PORT: &str = include_str!("fixtures/host_up_open_port.xml");
    const HOST_DOWN: &str = include_str!("fixtures/host_down.xml");
    const OS_DETECT_WITH_MATCHES: &str = include_str!("fixtures/os_detect_with_matches.xml");
    const REAL_NMAP_OUTPUT_WITH_DTD: &str = include_str!("fixtures/real_nmap_output_with_dtd.xml");
    const HOST_WITH_VULN_SCRIPTS: &str = include_str!("fixtures/host_with_vuln_scripts.xml");

    #[test]
    fn parses_host_up_with_open_and_closed_ports() {
        let report = parse_nmap_xml(HOST_UP_OPEN_PORT, false).expect("should parse");
        assert_eq!(report.target, "192.168.1.10");
        assert!(!report.os_detection);
        assert_eq!(report.hosts.len(), 1);
        let host = &report.hosts[0];
        assert_eq!(host.address, "192.168.1.10");
        assert!(host.up);
        assert_eq!(host.ports.len(), 2);
        assert_eq!(
            host.ports[0],
            PortReport {
                port: 22,
                protocol: "tcp".to_string(),
                state: "open".to_string(),
                service: Some("ssh".to_string()),
                scripts: vec![]
            }
        );
        assert_eq!(
            host.ports[1],
            PortReport {
                port: 80,
                protocol: "tcp".to_string(),
                state: "closed".to_string(),
                service: None,
                scripts: vec![]
            }
        );
        assert!(host.os_matches.is_empty());
        assert!(host.ports[0].scripts.is_empty());
        assert!(host.host_scripts.is_empty());
    }

    #[test]
    fn parses_host_down_as_legitimate_zero_result_not_error() {
        let report = parse_nmap_xml(HOST_DOWN, false).expect("host-down is not a parse error");
        assert_eq!(report.hosts.len(), 1);
        assert!(!report.hosts[0].up);
        assert!(report.hosts[0].ports.is_empty());
    }

    #[test]
    fn parses_os_detect_with_osmatches_and_filtered_port() {
        let report = parse_nmap_xml(OS_DETECT_WITH_MATCHES, true).expect("should parse");
        assert!(report.os_detection);
        let host = &report.hosts[0];
        assert_eq!(host.ports[0].state, "filtered");
        assert_eq!(
            host.os_matches,
            vec!["Linux 5.X".to_string(), "Linux 6.X".to_string()]
        );
    }

    #[test]
    fn parses_per_port_script_results() {
        let report = parse_nmap_xml(HOST_WITH_VULN_SCRIPTS, false).expect("should parse");
        let host = &report.hosts[0];
        let port_80 = host
            .ports
            .iter()
            .find(|p| p.port == 80)
            .expect("port 80 present");
        assert_eq!(port_80.scripts.len(), 2);
        assert_eq!(port_80.scripts[0].id, "http-csrf");
        assert_eq!(
            port_80.scripts[0].output,
            "Couldn't find any CSRF vulnerabilities."
        );
        assert_eq!(port_80.scripts[1].id, "http-vuln-cve2014-3704");
        let port_445 = host
            .ports
            .iter()
            .find(|p| p.port == 445)
            .expect("port 445 present");
        assert!(
            port_445.scripts.is_empty(),
            "port with no <script> children must have an empty scripts vec"
        );
    }

    #[test]
    fn parses_host_level_script_results_using_output_attribute_not_text_child() {
        let report = parse_nmap_xml(HOST_WITH_VULN_SCRIPTS, false).expect("should parse");
        let host = &report.hosts[0];
        assert_eq!(host.host_scripts.len(), 2);
        assert_eq!(host.host_scripts[0].id, "smb-vuln-ms10-054");
        // The fixture's <script> element has text child "false" AND an
        // output="false" attribute for this one — output attribute must win,
        // text child must be ignored (this is what proves it, not a
        // coincidence: the second script's output attribute is a long
        // sentence while ITS text child is also "false" — if the parser were
        // reading the text child instead of the attribute, this assertion
        // would fail).
        assert_eq!(host.host_scripts[1].id, "samba-vuln-cve-2012-1182");
        assert_eq!(
            host.host_scripts[1].output,
            "Could not negotiate a connection:SMB: Failed to receive bytes: EOF"
        );
    }

    #[test]
    fn malformed_xml_is_a_readable_parse_error_not_a_panic() {
        let err = parse_nmap_xml("<nmaprun><host", false).unwrap_err();
        assert!(matches!(err, ParseError::MalformedXml(_)));
        assert!(err.to_string().contains("XML nmap malformato"));
    }

    #[test]
    fn parses_real_nmap_output_including_doctype_and_stylesheet_pi() {
        // Captured live from a real `nmap -sT -oX -` run against 127.0.0.1
        // during Task 5's manual smoke test (Docs/superpowers/sdd/task-5).
        // Real nmap -oX output always starts with `<!DOCTYPE nmaprun>` (no
        // internal/external subset, but still a DTD declaration) plus an
        // `<?xml-stylesheet ...?>` processing instruction and an XML comment
        // with entity references — none of which Task 1's hand-written
        // fixtures included. `roxmltree::Document::parse` rejects ANY DTD
        // declaration by default (`ParsingOptions::allow_dtd` defaults to
        // `false`), so this test failed against every real nmap scan on
        // every machine until this fix — a crate-breaking bug undetected
        // until this task's mandated end-to-end smoke check.
        let report = parse_nmap_xml(REAL_NMAP_OUTPUT_WITH_DTD, false)
            .expect("real nmap -oX output (with DOCTYPE) must parse, not be rejected as malformed");
        assert_eq!(report.target, "127.0.0.1");
        assert_eq!(report.hosts.len(), 1);
        let host = &report.hosts[0];
        assert!(host.up);
        assert_eq!(host.address, "127.0.0.1");
        assert_eq!(host.ports.len(), 3);
        assert_eq!(host.ports[0].port, 135);
        assert_eq!(host.ports[0].service.as_deref(), Some("msrpc"));
    }

    #[test]
    fn empty_hosts_list_when_no_host_elements_present() {
        let report = parse_nmap_xml(
            r#"<?xml version="1.0"?><nmaprun args="nmap -sT -oX - 1.2.3.4"></nmaprun>"#,
            false,
        )
        .expect("valid XML with no <host> elements is not a parse error");
        assert!(report.hosts.is_empty());
        assert_eq!(report.target, "1.2.3.4");
    }
}
