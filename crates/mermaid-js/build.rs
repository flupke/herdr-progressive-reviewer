//! Compresses the vendored Mermaid once, at build time: the binary holds the gzipped file only.

use std::io::Write;
use std::path::PathBuf;

fn main() {
    let source = "vendor/mermaid.min.js";
    println!("cargo::rerun-if-changed={source}");
    let script = std::fs::read(source).expect("the vendored Mermaid is readable");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&script).expect("gzip writes to memory");
    let gzipped = encoder.finish().expect("gzip writes to memory");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(out.join("mermaid.min.js.gz"), gzipped).expect("OUT_DIR is writable");
}
