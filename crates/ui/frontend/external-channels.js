// external-channels.js — registro dei canali tool esterni con finestra
// dedicata. Mirror lato frontend di crates/orchestrator/src/external_channel.rs
// (registro Rust). Da Task 5 (2026-07-16) contiene il primo canale reale
// (nmap) — vedi Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md.
// Da Task 3 dell'infrastruttura pytools (2026-07-21) contiene anche
// "python-ping", canale di prova per validare lo spawn di un server MCP
// Python — vedi Docs/superpowers/specs/2026-07-21-pytools-infrastructure-design.md.
// Da Task 11 (2026-07-22) contiene anche "financial-markets", primo tool
// Python reale del dominio — vedi Docs/superpowers/specs/2026-07-22-
// financial-markets-stock-report-design.md.
//
// Responsibility (SRP): pura lookup, nessuna dipendenza da DOM/Tauri — mirror
// di search-buffer.js.

/** @typedef {{id: string, slashTrigger: string, windowTitle: string}} ExternalChannel */

/** @type {ExternalChannel[]} */
export const EXTERNAL_TOOL_CHANNELS = [
  { id: "netsec", slashTrigger: "/netsec", windowTitle: "Lare — netsec" },
  { id: "python-ping", slashTrigger: "/pyping", windowTitle: "Lare — Python ping" },
  { id: "financial-markets", slashTrigger: "/markets", windowTitle: "Lare — Financial Markets" },
];

/**
 * Trova il canale il cui slashTrigger combacia con l'input (dopo trim),
 * o null se nessuno lo rivendica.
 * @param {string} input
 * @param {ExternalChannel[]} registry
 * @returns {ExternalChannel|null}
 */
export function findExternalChannelBySlash(input, registry) {
  const trimmed = input.trim();
  return registry.find((c) => c.slashTrigger === trimmed) ?? null;
}
