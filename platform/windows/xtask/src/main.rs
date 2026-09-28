//! Write `platform/windows/registry_keys.nsh` from the shell registry table.

use std::path::PathBuf;

fn main() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../registry_keys.nsh");
    let text = dither_shell::emit_nsh();
    std::fs::write(&path, &text).expect("write nsh");
    println!("wrote {}", path.display());
}
