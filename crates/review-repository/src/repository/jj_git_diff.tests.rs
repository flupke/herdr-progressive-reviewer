use std::collections::BTreeSet;

use super::JjGitDiffParser;
use crate::repository::RepoPath;

#[test]
fn paths_decode_jj_quoted_bytes() {
    assert_eq!(
        JjGitDiffParser::decode_path(br#""src/line\nquote\"slash\\byte\377.rs""#).unwrap(),
        b"src/line\nquote\"slash\\byte\xff.rs"
    );
    assert_eq!(
        JjGitDiffParser::decode_path(b"src/path with spaces.rs").unwrap(),
        b"src/path with spaces.rs"
    );
}

#[test]
fn parser_ignores_only_the_synthetic_description() {
    let changed = RepoPath::from_bytes(b"src/changed file.rs".to_vec());
    let description_file = RepoPath::from_bytes(b"JJ-COMMIT-DESCRIPTION".to_vec());
    let planned = BTreeSet::from([changed.clone(), description_file.clone()]);
    let output = b"diff --git a/JJ-COMMIT-DESCRIPTION b/JJ-COMMIT-DESCRIPTION\n--- JJ-COMMIT-DESCRIPTION\n+++ JJ-COMMIT-DESCRIPTION\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/src/changed file.rs b/src/changed file.rs\n--- a/src/changed file.rs\n+++ b/src/changed file.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/JJ-COMMIT-DESCRIPTION b/JJ-COMMIT-DESCRIPTION\n--- a/JJ-COMMIT-DESCRIPTION\n+++ b/JJ-COMMIT-DESCRIPTION\n@@ -1 +1 @@\n-old\n+new\n";

    assert_eq!(
        JjGitDiffParser::new(output, &planned).parse().unwrap(),
        BTreeSet::from([changed, description_file])
    );
    assert!(
        JjGitDiffParser::new(
            b"diff --git a/src/unplanned.rs b/src/unplanned.rs\n",
            &planned
        )
        .parse()
        .is_err()
    );
}

#[test]
fn parser_matches_mode_only_changes_with_spaces() {
    let changed = RepoPath::from_bytes(b"mode only file.rs".to_vec());
    let planned = BTreeSet::from([changed.clone()]);
    let output =
        b"diff --git a/mode only file.rs b/mode only file.rs\nold mode 100644\nnew mode 100755\n";

    assert_eq!(
        JjGitDiffParser::new(output, &planned).parse().unwrap(),
        BTreeSet::from([changed])
    );
}

#[test]
fn parser_matches_mode_only_changes_with_quoted_bytes() {
    let changed = RepoPath::from_bytes(b"mode \"only\".rs".to_vec());
    let planned = BTreeSet::from([changed.clone()]);
    let output = b"diff --git \"a/mode \\\"only\\\".rs\" \"b/mode \\\"only\\\".rs\"\nold mode 100644\nnew mode 100755\n";

    assert_eq!(
        JjGitDiffParser::new(output, &planned).parse().unwrap(),
        BTreeSet::from([changed])
    );
}

#[test]
fn paths_reject_invalid_escapes() {
    assert!(JjGitDiffParser::decode_path(br#""src/bad\q.rs""#).is_err());
    assert!(JjGitDiffParser::decode_path(br#""src/large\777.rs""#).is_err());
    assert!(JjGitDiffParser::decode_path(br#""src/missing-quote.rs"#).is_err());
}
