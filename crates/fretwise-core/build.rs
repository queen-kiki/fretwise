//! Embeds every `scales/*.json` into the binary, so the app (and a plugin
//! later) needs no files at runtime. Writes `$OUT_DIR/builtin_scales.rs`, a
//! list of `(file name, contents)` pairs for `scale.rs` to parse.

use std::path::Path;
use std::{env, fs};

fn main() {
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("scales");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut files: Vec<_> = fs::read_dir(&dir)
        .expect("scales folder")
        .map(|e| e.expect("scales folder entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();

    let mut out = String::from("&[\n");
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path.file_name().unwrap().to_string_lossy();
        out += &format!("    ({name:?}, include_str!({:?})),\n", path.display().to_string());
    }
    out += "]\n";
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("builtin_scales.rs"), out).unwrap();
}
