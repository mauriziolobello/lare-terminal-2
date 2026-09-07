// build.rs — dice a Cargo di ricompilare `ui` quando cambia SOLO il frontend
// (../frontend). Senza questo, `generate_context!` in main.rs incorpora
// `frontendDist` a compile time e una modifica a terminal.js/host.js/*.mjs
// non fa ripartire la build: bisognava fare `cargo clean -p ui` a mano
// (gotcha scoperto durante lo spike 2, Docs/i18n/ita/spikes/2026-09-05-lare-terminal-window.md).
fn main() {
    tauri_build::build();
    println!("cargo:rerun-if-changed=../frontend");
}
