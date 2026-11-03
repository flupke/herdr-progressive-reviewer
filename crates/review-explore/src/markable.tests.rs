use super::*;

#[test]
fn a_longer_abbreviation_of_the_same_objects_is_the_same_diff() {
    let captured =
        b"diff --git a/a.rs b/a.rs\nindex 1a2b3c4..5d6e7f8 100644\n@@ -1 +1 @@\n-a\n+b\n";
    let current =
        b"diff --git a/a.rs b/a.rs\nindex 1a2b3c4d..5d6e7f80 100644\n@@ -1 +1 @@\n-a\n+b\n";

    assert!(same_diff(current, captured));
}

#[test]
fn another_object_or_another_line_is_another_diff() {
    let captured = b"index 1a2b3c4..5d6e7f8 100644\n@@ -1 +1 @@\n+b\n";

    assert!(!same_diff(
        b"index 1a2b3c4..5d6e7f9 100644\n@@ -1 +1 @@\n+b\n",
        captured
    ));
    assert!(!same_diff(
        b"index 1a2b3c4..5d6e7f8 100755\n@@ -1 +1 @@\n+b\n",
        captured
    ));
    assert!(!same_diff(
        b"index 1a2b3c4..5d6e7f8 100644\n@@ -2 +2 @@\n+b\n",
        captured
    ));
}
