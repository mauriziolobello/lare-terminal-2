// build.rs standard di Tauri 2: genera in fase di build le risorse Windows
// (icona .exe da icons/icon.ico) e collega tauri.conf.json/capabilities al
// binario. Identico, riga per riga, a quello del crate v1 (vedi
// crates/ui/src-tauri/build.rs nel progetto v1, letto come riferimento).
fn main() {
    tauri_build::build()
}
