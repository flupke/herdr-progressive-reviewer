//! Opening a review split must synchronize its PTY before further user input.

use std::process::Command;

#[test]
fn newly_opened_review_pane_fits_its_split_without_interactive_resize() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/pane_geometry.py"
        ))
        .arg(directory.path())
        .arg(env!("CARGO_BIN_EXE_reviewer-control"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
