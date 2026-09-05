//! # mcp-server
//!
//! MCP server for Lare Terminal (Contratto B, `03-protocol.md`).
//!
//! ## Five-level architecture (SRP)
//!
//! 1. **`system`** — pure, testable core (legacy one-shot runner, kept for reference).
//! 2. **`session`** — persistent shell session: `run(command) -> CommandOutput`.
//! 3. **`open_target`** — classificazione e apertura di target nativi (ADR-012).
//! 4. **`routines`** — repository di script PowerShell riusabili: indice
//!    JSON, ricerca, costruzione invocazione (vedi doc-comment del modulo).
//! 5. **`main.rs`** — thin MCP glue: serves `run_in_session`, `reset_session`,
//!    `open_target`, `search_routines`, `run_routine` via stdio.

pub mod open_target;
pub mod routines;
pub mod session;
pub mod system;
