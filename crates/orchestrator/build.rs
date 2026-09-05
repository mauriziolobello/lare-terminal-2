//! build.rs — reads sibling crates' `Cargo.toml` versions at build time and
//! exposes them as compile-time env vars, so `main.rs`'s startup log can
//! show `protocol`/`plugin-protocol`/`mcp-server`/`mcp-nmap`'s versions
//! alongside orchestrator's own (2026-07-19, user request) without adding
//! `mcp-server`/`mcp-nmap` as Cargo dependencies — orchestrator spawns them
//! as separate processes and deliberately never links against them; this
//! keeps that boundary intact while still surfacing accurate version info.
//!
//! A plain text scan (no `toml` crate) is enough: every `Cargo.toml` in
//! this workspace has a standard `[package]` section with a `version = "x.y.z"`
//! line near the top, before any `[dependencies]` table.

use std::fs;

/// Extracts the `version = "..."` value from a `Cargo.toml`'s `[package]`
/// section specifically — NOT any `version = "..."` that might appear
/// inside a `[dependencies]` table (e.g. `tokio = { version = "1", ... }`),
/// by tracking which `[section]` we're currently inside and only matching
/// while inside `[package]`.
fn extract_package_version(cargo_toml_path: &str) -> String {
    let content = fs::read_to_string(cargo_toml_path)
        .unwrap_or_else(|e| panic!("build.rs: failed to read {cargo_toml_path}: {e}"));

    let mut in_package_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" {
            in_package_section = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_package_section = false;
            continue;
        }
        if in_package_section && trimmed.starts_with("version") {
            if let Some(eq_idx) = trimmed.find('=') {
                let value = trimmed[eq_idx + 1..].trim().trim_matches('"');
                return value.to_string();
            }
        }
    }
    panic!("build.rs: no [package] version found in {cargo_toml_path}");
}

fn main() {
    let siblings = [
        ("PROTOCOL_VERSION", "../protocol/Cargo.toml"),
        ("PLUGIN_PROTOCOL_VERSION", "../plugin-protocol/Cargo.toml"),
        ("MCP_SERVER_VERSION", "../mcp-server/Cargo.toml"),
        ("MCP_NMAP_VERSION", "../mcp-nmap/Cargo.toml"),
    ];
    for (env_name, path) in siblings {
        let version = extract_package_version(path);
        println!("cargo:rustc-env={env_name}={version}");
        // Re-run this build script (and therefore refresh the baked-in
        // version) whenever any sibling's own Cargo.toml changes — without
        // this, a version bump in e.g. mcp-nmap wouldn't be picked up by
        // orchestrator's next build unless something else also touched a
        // file cargo already watches.
        println!("cargo:rerun-if-changed={path}");
    }
}
