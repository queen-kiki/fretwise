//! Embeds every `scales/*.json` and `chords/*.json` into the binary, so the app
//! (and a plugin later) needs no files at runtime. Writes
//! `$OUT_DIR/builtin_scales.rs` and `$OUT_DIR/builtin_chords.rs`, lists of
//! `(file name, contents)` pairs for `scale.rs` and `chord.rs` to parse.

use std::path::Path;
use std::{env, fs};

fn embed_dir(folder: &str, out_file: &str) {
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join(folder);
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{folder} folder: {e}"))
        .map(|e| e.expect("folder entry").path())
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
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join(out_file), out).unwrap();
}

fn main() {
    embed_dir("scales", "builtin_scales.rs");
    embed_dir("chords", "builtin_chords.rs");
    embed_dir("decks", "builtin_decks.rs");
}
