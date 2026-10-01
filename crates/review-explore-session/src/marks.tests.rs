use review_hunks::LineSelection;
use review_repository::repository::{ChangedFile, RepoPath};
use review_source::SourceLineRange;
use review_types::MarkAuthor;

use super::*;

fn at(side: SourceSide, lines: Option<(u32, u32)>) -> CodeLocation {
    CodeLocation {
        path: RepoPath::from_bytes(b"src/lib.rs"),
        side,
        lines: lines.map(|(first_line, last_line)| SourceLineRange {
            first_line,
            last_line,
        }),
    }
}

#[test]
fn marks_name_zero_based_lines_on_their_side() {
    let marks = [
        at(SourceSide::Old, Some((2, 3))),
        at(SourceSide::New, Some((5, 5))),
    ];

    let selection = FileRequest::selection(&marks.iter().collect::<Vec<_>>(), || unreachable!());

    assert_eq!(selection.removed, [1, 2].into());
    assert_eq!(selection.added, [4].into());
}

#[test]
fn a_whole_file_mark_names_every_line() {
    let marks = [at(SourceSide::New, None)];
    let every = LineSelection {
        removed: [0].into(),
        added: [0, 1].into(),
    };

    assert_eq!(
        FileRequest::selection(&marks.iter().collect::<Vec<_>>(), || every.clone()),
        every
    );
}

#[test]
fn applied_lines_read_back_as_one_based_runs() {
    let file = ChangedFile::modified("src/lib.rs");
    let selection = LineSelection {
        removed: [1].into(),
        added: [4, 5, 9].into(),
    };

    assert_eq!(
        FileChange::locations(&file, &selection),
        [
            at(SourceSide::Old, Some((2, 2))),
            at(SourceSide::New, Some((5, 6))),
            at(SourceSide::New, Some((10, 10))),
        ]
    );
}

#[test]
fn reopened_lines_keep_who_had_marked_them() {
    let file = ChangedFile::modified("src/lib.rs");
    let before = ReviewedLines {
        removed: [].into(),
        added: [(3, MarkAuthor::Jev), (4, MarkAuthor::Reviewer)].into(),
    };

    let reopened = FileChange::reopened(&file, &before, &ReviewedLines::default());

    assert_eq!(
        reopened,
        [
            ReopenedLines {
                location: at(SourceSide::New, Some((4, 4))),
                author: MarkAuthor::Jev,
            },
            ReopenedLines {
                location: at(SourceSide::New, Some((5, 5))),
                author: MarkAuthor::Reviewer,
            },
        ]
    );
}

#[test]
fn a_line_taken_over_from_another_author_is_reopened_and_marked_again() {
    let file = ChangedFile::modified("src/lib.rs");
    let explore = MarkAuthor::Explore {
        answer: "answer".into(),
    };
    let lines = |author: MarkAuthor| FileLines {
        open: Vec::new(),
        reviewed: ReviewedLines {
            removed: [].into(),
            added: [(14, author)].into(),
        },
    };

    let change = FileChange::between(
        &file,
        &lines(MarkAuthor::Jev),
        &lines(explore.clone()),
        &explore,
    );

    assert_eq!(change.reviewed, [at(SourceSide::New, Some((15, 15)))]);
    assert_eq!(
        change.reopened,
        [ReopenedLines {
            location: at(SourceSide::New, Some((15, 15))),
            author: MarkAuthor::Jev,
        }]
    );
}
