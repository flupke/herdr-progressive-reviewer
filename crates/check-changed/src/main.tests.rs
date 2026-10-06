use super::summary_paths;

#[test]
fn a_rename_counts_both_its_paths() {
    assert_eq!(
        summary_paths("R crates/a/{src => tests}/lib.rs"),
        ["crates/a/src/lib.rs", "crates/a/tests/lib.rs"]
    );
    assert_eq!(
        summary_paths("R {crates/a => docs}/notes.txt"),
        ["crates/a/notes.txt", "docs/notes.txt"]
    );
    assert_eq!(summary_paths("R a.txt => b.txt"), ["a.txt", "b.txt"]);
}

#[test]
fn a_change_counts_its_path() {
    assert_eq!(summary_paths("M Cargo.lock"), ["Cargo.lock"]);
    assert_eq!(summary_paths("D docs/old.md"), ["docs/old.md"]);
}
