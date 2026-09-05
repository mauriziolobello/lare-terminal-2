// lib.rs — Library entry point for the `ui` crate.
//
// Exposes modules that contain testable pure logic so they can be reached by
// `cargo test -p ui` without spinning up a Tauri runtime.
//
// The binary (`main.rs`) re-uses these modules via `use ui_lib::config`.

pub mod config;
pub mod archive;
pub mod library_watch;
