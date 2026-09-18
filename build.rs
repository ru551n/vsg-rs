//! Embeds the `ieee` and `std` VHDL sources (`vendor/vhdl_libraries`) in the binary.
//!
//! Semantic analysis cannot resolve `ieee.std_logic_1164` without them, and vsg-rs ships as one
//! self-contained file, so the sources travel inside it and are written to a cache directory on
//! first use (see `src/semantic.rs`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/vhdl_libraries");
    println!("cargo:rerun-if-changed=vendor/vhdl_libraries");
    let mut paths = Vec::new();
    files(&root, &mut paths);
    paths.sort();

    let mut code = String::from("pub static VHDL_LIBRARIES: &[(&str, &[u8])] = &[\n");
    for path in &paths {
        let name = path
            .strip_prefix(&root)
            .expect("walked below the library root")
            .to_string_lossy()
            .replace('\\', "/");
        // The sources are Latin-1, not UTF-8 (`std/standard.vhd` has a non-UTF-8 byte), so they
        // travel as bytes and are written back out unchanged.
        let _ = writeln!(
            code,
            "    ({:?}, include_bytes!({:?})),",
            name,
            path.to_string_lossy()
        );
    }
    code += "];\n";

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"))
        .join("vhdl_libraries.rs");
    std::fs::write(&out, code).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
}
