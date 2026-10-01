use super::*;

fn path(text: &str) -> RepoPath {
    RepoPath::from_bytes(text.as_bytes())
}

fn whole(path: &str, changed: u64) -> UnreviewedFile {
    UnreviewedFile {
        path: self::path(path),
        lines: UnreviewedLines::Whole { changed },
    }
}

#[test]
fn wholly_open_directories_collapse_and_partial_files_list_their_ranges() {
    let unreviewed = Unreviewed {
        paths: [
            "crates/store/a.rs",
            "crates/store/b.rs",
            "src/session.rs",
            "src/store.rs",
            "src/done.rs",
            "assets/logo.bin",
        ]
        .into_iter()
        .map(path)
        .collect(),
        files: vec![
            whole("crates/store/a.rs", 10),
            whole("crates/store/b.rs", 5),
            UnreviewedFile {
                path: path("src/session.rs"),
                lines: UnreviewedLines::Ranges(vec![
                    UnreviewedRange {
                        side: SourceSide::New,
                        lines: SourceLineRange {
                            first_line: 40,
                            last_line: 62,
                        },
                        since_review: false,
                    },
                    UnreviewedRange {
                        side: SourceSide::Old,
                        lines: SourceLineRange {
                            first_line: 10,
                            last_line: 10,
                        },
                        since_review: true,
                    },
                ]),
            },
            whole("src/store.rs", 3),
            whole("assets/logo.bin", 0),
        ],
        jev: None,
        status: UnreviewedStatus::default(),
    };

    assert_eq!(
        unreviewed.to_string(),
        "\nUnreviewed lines: 42 changed lines in 5 files (old = base lines, new = current lines)\n\
         \x20 assets/logo.bin  whole file\n\
         \x20 crates/          all 2 files · 15 lines\n\
         \x20 src/session.rs   new 40-62, old 10 (since review)\n\
         \x20 src/store.rs     whole file · 3 lines\n"
    );
}

#[test]
fn a_fully_reviewed_checkpoint_says_so() {
    let unreviewed = Unreviewed {
        jev: Some("marked 2 files reviewed.".into()),
        ..Unreviewed::default()
    };

    assert_eq!(
        unreviewed.to_string(),
        "\nJev: marked 2 files reviewed.\n\nUnreviewed lines: none; every changed line is reviewed.\n"
    );
}

#[test]
fn unreadable_lines_say_why() {
    let unreviewed = Unreviewed {
        status: UnreviewedStatus::Unavailable("the repository is not ready".into()),
        ..Unreviewed::default()
    };

    assert_eq!(
        unreviewed.to_string(),
        "\nUnreviewed lines: unavailable: the repository is not ready\n"
    );
}
