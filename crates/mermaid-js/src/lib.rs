//! Mermaid, the JavaScript library that draws the diagrams of the agent's Markdown on the Explore
//! page, pinned to one version and built into the binary, so the page needs no network.
//!
//! The vendored file is `vendor/mermaid.min.js` (see `vendor/README.md`); the binary holds it
//! gzipped, as the page serves it.

use std::io::Read;
use std::sync::LazyLock;

#[cfg(test)]
mod tests;

/// The pinned version, as a literal for `concat!`.
macro_rules! version {
    () => {
        "11.17.2"
    };
}

/// The pinned version of Mermaid.
pub const VERSION: &str = version!();

/// The language of a fenced Markdown block that holds a diagram: ```` ```mermaid ````.
pub const FENCE: &str = "mermaid";

/// The name the page serves the script under. It changes with the version, so a browser may keep
/// the file for good.
pub const FILE_NAME: &str = concat!("mermaid-", version!(), ".min.js");

/// The script, gzipped.
pub const GZIPPED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mermaid.min.js.gz"));

/// The script, for a client that does not accept gzip: decompressed once, on first use.
pub fn script() -> &'static [u8] {
    static SCRIPT: LazyLock<Vec<u8>> = LazyLock::new(|| {
        let mut script = Vec::new();
        flate2::read::GzDecoder::new(GZIPPED)
            .read_to_end(&mut script)
            .expect("the build script wrote valid gzip");
        script
    });
    &SCRIPT
}
