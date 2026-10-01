use super::*;

#[test]
fn equal_versions_have_no_diff() {
    assert!(unified_diff("f", b"same\n", b"same\n").is_empty());
}

#[test]
fn nearby_changes_share_one_hunk_with_git_context() {
    let before = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n16\n";
    let after = b"1\n2\nthree\n4\n5\n6\n7\neight\n9\n10\n11\n12\n13\n14\n15\nsixteen\n";

    assert_eq!(
        String::from_utf8(unified_diff("src/a.rs", before, after)).unwrap(),
        "diff --git a/src/a.rs b/src/a.rs\n\
         --- a/src/a.rs\n\
         +++ b/src/a.rs\n\
         @@ -1,11 +1,11 @@\n \
         1\n \
         2\n\
         -3\n\
         +three\n \
         4\n \
         5\n \
         6\n \
         7\n\
         -8\n\
         +eight\n \
         9\n \
         10\n \
         11\n\
         @@ -13,4 +13,4 @@\n \
         13\n \
         14\n \
         15\n\
         -16\n\
         +sixteen\n"
    );
}

#[test]
fn a_missing_final_newline_is_marked_like_git() {
    assert_eq!(
        String::from_utf8(unified_diff("f", b"a\nb", b"a\nc\n")).unwrap(),
        "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n"
    );
}

#[test]
fn an_empty_side_names_the_line_before_the_hunk() {
    assert_eq!(
        String::from_utf8(unified_diff("f", b"", b"new\n")).unwrap(),
        "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -0,0 +1 @@\n+new\n"
    );
}

#[test]
fn reverse_application_rebuilds_the_old_version() {
    let pairs: [(&[u8], &[u8]); 7] = [
        (b"a\nb\nc\n", b"a\nB\nc\n"),
        (b"", b"new\nfile\n"),
        (b"old\nfile\n", b""),
        (b"a\nb", b"a\nb\n"),
        (b"a\nb\n", b"a\nb"),
        (b"x\r\ny\r\n", b"x\r\nY\r\n"),
        (
            b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n",
            b"0\n1\n2\n3\n4\n5\n7\n8\n9\n10\n11\ntwelve",
        ),
    ];
    for (before, after) in pairs {
        let diff = unified_diff("f", before, after);

        assert_eq!(
            reverse_apply(&diff, after).as_deref(),
            Some(before),
            "{}",
            String::from_utf8_lossy(&diff)
        );
    }
}

#[test]
fn reverse_application_rejects_a_diff_of_another_version() {
    let diff = unified_diff("f", b"a\nb\n", b"a\nB\n");

    assert_eq!(reverse_apply(&diff, b"a\nC\n"), None);
}

#[test]
fn reverse_application_of_no_diff_keeps_the_new_version() {
    assert_eq!(
        reverse_apply(b"", b"kept\n").as_deref(),
        Some(&b"kept\n"[..])
    );
}
