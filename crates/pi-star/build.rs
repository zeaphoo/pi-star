use std::path::Path;

fn main() {
    // Rebuild when the pre-built UI changes. rust-embed reads the directory at
    // compile time for release builds; tracking each file keeps `cargo build`
    // honest even when Vite rewrites a file in place.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    println!("cargo:rerun-if-changed={}", root.display());
    emit_tree(&root);
}

fn emit_tree(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            emit_tree(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
