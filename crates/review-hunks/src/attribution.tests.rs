use review_types::MarkAuthor;

use super::*;

#[test]
fn only_changed_lines_keep_an_author_and_runs_merge() {
    let mut rebuild = Rebuild::with_default(MarkAuthor::Reviewer);
    for (line, author) in [
        (&b"a\n"[..], MarkAuthor::Jev),
        (b"X\n", MarkAuthor::Jev),
        (b"Y\n", MarkAuthor::Jev),
        (b"Z\n", MarkAuthor::Reviewer),
        (b"b\n", MarkAuthor::Jev),
    ] {
        rebuild.push(line, &author);
    }
    rebuild.remove(1, &MarkAuthor::Jev);
    rebuild.remove(0, &MarkAuthor::Jev);

    let reviewed = rebuild.finish(b"a\nold\nb\n");

    assert_eq!(reviewed.text, b"a\nX\nY\nZ\nb\n");
    assert_eq!(
        reviewed.attribution.added,
        [AuthoredLines {
            lines: 1..3,
            author: MarkAuthor::Jev,
        }]
    );
    // Base line 0 is still there, so only the removal of line 1 counts.
    assert_eq!(
        reviewed.attribution.removed,
        [AuthoredLines {
            lines: 1..2,
            author: MarkAuthor::Jev,
        }]
    );
    assert_eq!(reviewed.attribution.added_by(3), &MarkAuthor::Reviewer);
    assert_eq!(reviewed.attribution.uniform_author(), None);
}

#[test]
fn one_author_of_every_change_becomes_the_default() {
    let mut rebuild = Rebuild::with_default(MarkAuthor::Reviewer);
    rebuild.push(b"X\n", &MarkAuthor::Jev);
    rebuild.remove(0, &MarkAuthor::Jev);

    let reviewed = rebuild.finish(b"old\n");

    assert_eq!(reviewed.attribution, Attribution::uniform(MarkAuthor::Jev));
}

#[test]
fn lines_of_the_default_author_need_no_entry() {
    let mut rebuild = Rebuild::with_default(MarkAuthor::Jev);
    rebuild.push(b"X\n", &MarkAuthor::Jev);

    let reviewed = rebuild.finish(b"");

    assert_eq!(reviewed.attribution, Attribution::uniform(MarkAuthor::Jev));
    assert_eq!(
        reviewed.attribution.uniform_author(),
        Some(&MarkAuthor::Jev)
    );
}
