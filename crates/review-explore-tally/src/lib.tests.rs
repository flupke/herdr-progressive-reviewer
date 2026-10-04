//! The numbers of the meter, from review marks the tests write down line by line.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use review_explore::{
    CodeLocation, Comparison, Exploration, NotRelevantMark, ReviewerAnswer, SourceSide, TurnMarks,
};
use review_hunks::{ChangedLines, ReviewedLines};
use review_repository::repository::{ChangedFile, RepoPath};
use review_source::{ReviewCheckpoint, SourceLineRange};
use review_state::{FileLines, OpenLines};
use review_types::MarkAuthor;

use super::*;

const FILE: &str = "src/queue.rs";

/// One file's lines: `open` lines on each side, and reviewed lines by author.
struct Lines {
    open_removed: Vec<u32>,
    open_added: Vec<u32>,
    reviewed: ReviewedLines,
}

impl Lines {
    fn open(removed: impl IntoIterator<Item = u32>, added: impl IntoIterator<Item = u32>) -> Self {
        Self {
            open_removed: removed.into_iter().collect(),
            open_added: added.into_iter().collect(),
            reviewed: ReviewedLines::default(),
        }
    }

    fn added_by(mut self, lines: impl IntoIterator<Item = u32>, author: &MarkAuthor) -> Self {
        for line in lines {
            self.reviewed.added.insert(line, author.clone());
        }
        self
    }

    fn removed_by(mut self, lines: impl IntoIterator<Item = u32>, author: &MarkAuthor) -> Self {
        for line in lines {
            self.reviewed.removed.insert(line, author.clone());
        }
        self
    }

    fn of(self, path: &str) -> FileMarks {
        FileMarks {
            file: ChangedFile::modified(path),
            marks: Marks::Lines(FileLines {
                open: vec![OpenLines {
                    lines: ChangedLines {
                        removed: self.open_removed,
                        added: self.open_added,
                        rewrites_reviewed: false,
                    },
                    since_review: false,
                }],
                reviewed: self.reviewed,
            }),
        }
    }
}

fn location(path: &str, side: SourceSide, lines: Option<(u32, u32)>) -> CodeLocation {
    CodeLocation {
        path: RepoPath::from_bytes(path.as_bytes()),
        side,
        lines: lines.map(|(first_line, last_line)| SourceLineRange {
            first_line,
            last_line,
        }),
    }
}

fn not_relevant(location: CodeLocation) -> NotRelevantMark {
    NotRelevantMark {
        location,
        reason: None,
        test: None,
    }
}

fn explore(answer: &str) -> MarkAuthor {
    MarkAuthor::Explore {
        answer: answer.into(),
    }
}

/// A round whose reviewer gave `answers`, and whose turns marked `marks`, by request.
fn round(
    answers: &[&str],
    marks: impl IntoIterator<Item = (&'static str, TurnMarks)>,
) -> ExploreRound {
    let checkpoint = ReviewCheckpoint::new("unit", "commit");
    let mut exploration = Exploration::new(Arc::new(Comparison {
        repository_root: PathBuf::from("/repository"),
        checkpoint: checkpoint.clone(),
        files: Vec::new(),
        context: Vec::new(),
        diffs: Vec::new(),
        manifest: Vec::new(),
        sources: Vec::new(),
        base: None,
    }));
    exploration.answers = answers
        .iter()
        .map(|id| ReviewerAnswer {
            id: (*id).into(),
            checkpoint: checkpoint.clone(),
            question: None,
            in_reply_to: format!("request of {id}"),
            option: None,
            text: String::new(),
            author: "reviewer".into(),
            first_pick: None,
        })
        .collect();
    ExploreRound {
        revision: 1,
        exploration,
        last_agent_session: None,
        turns: BTreeMap::new(),
        implementations: BTreeMap::new(),
        completion: None,
        marks: marks
            .into_iter()
            .map(|(request, marks)| (request.to_owned(), marks))
            .collect(),
    }
}

/// The agent's turn that posts a question citing `cited`, and asks to mark `reviewed`,
/// `not_relevant` and to reopen `reopened` once it is answered.
fn question(
    cited: &[CodeLocation],
    reviewed: Vec<CodeLocation>,
    not_relevant: Vec<NotRelevantMark>,
    reopened: Vec<CodeLocation>,
) -> InterviewUpdate {
    let evidence: Vec<_> = cited
        .iter()
        .map(|location| {
            let mut evidence = serde_json::to_value(location).unwrap();
            evidence["notes"] = "Evidence.".into();
            evidence
        })
        .collect();
    let mut update: InterviewUpdate = serde_json::from_value(serde_json::json!({
        "instance": "round", "request": "request", "checkpoint": {"review_unit": "unit", "checkpoint": "commit"},
        "interpretation": null, "reply": null, "topics": [],
        "next": {"id": "q2", "version": 1, "topic": "topic", "text": "Flush on each reply?",
            "rationale": null, "visual": null,
            "alternatives": [{"id": "keep", "text": "Keep it", "outcome": "accepted"}],
            "evidence": evidence},
        "conclusion": null, "limitations": [], "findings": []
    }))
    .unwrap();
    update.reviewed = reviewed;
    update.not_relevant = not_relevant;
    update.reopened = reopened;
    update
}

#[test]
fn each_marked_line_counts_for_who_marked_it() {
    let round = round(
        &["a1"],
        [
            (
                "request of a1",
                TurnMarks {
                    answer: Some("a1".into()),
                    not_relevant: vec![not_relevant(location(FILE, SourceSide::New, Some((3, 3))))],
                    ..TurnMarks::default()
                },
            ),
            ("kickoff", TurnMarks::default()),
        ],
    );
    let file = Lines::open([0], [9])
        .added_by([0, 1], &explore("a1"))
        .added_by([2], &explore("a1"))
        .added_by(
            [3],
            &MarkAuthor::ExploreRead {
                request: "kickoff".into(),
            },
        )
        .added_by([4, 5], &MarkAuthor::Jev)
        .added_by([6], &MarkAuthor::Reviewer)
        .added_by([7], &explore("an answer of an earlier round"))
        .removed_by(
            [1],
            &MarkAuthor::ExploreRead {
                request: "a turn of an earlier round".into(),
            },
        )
        .of(FILE);

    let tally = MarkTally::of(&[file], Some(&round), None);

    let expected = MarkedLines {
        answers: 2,
        jev: 2,
        not_relevant: 2,
        by_hand: 1,
        other_rounds: 2,
    };
    assert_eq!(tally.change.marked, expected);
    assert_eq!(tally.files[0].tally.marked, expected);
    assert_eq!(
        tally.change.changed,
        DiffStatistics {
            lines_added: 9,
            lines_removed: 2
        }
    );
    assert_eq!(tally.change.left, 2);
    assert_eq!(
        tally.change.share,
        Share {
            marked: 9,
            changed: 11,
            percent: 81
        }
    );
    assert_eq!(tally.gain, None);
}

#[test]
fn without_a_round_every_explore_mark_is_another_rounds() {
    let file = Lines::open([], [])
        .added_by([0], &explore("a1"))
        .added_by(
            [1],
            &MarkAuthor::ExploreRead {
                request: "kickoff".into(),
            },
        )
        .of(FILE);

    let tally = MarkTally::of(&[file], None, None);

    assert_eq!(tally.change.marked.other_rounds, 2);
    assert_eq!(tally.change.share.percent, 100);
}

#[test]
fn the_question_adds_the_open_lines_its_answer_marks() {
    // The reference capture's numbers: 135 changed lines, 52 marked, the question marks 15.
    // Its "39% → 50%" rounds to the nearest; the reviewer rounds every share down.
    let round = round(&["a1"], [("kickoff", TurnMarks::default())]);
    let queue = Lines::open(0..10, 52..125)
        .added_by(0..29, &explore("a1"))
        .added_by(29..47, &MarkAuthor::Jev)
        .added_by(
            47..52,
            &MarkAuthor::ExploreRead {
                request: "kickoff".into(),
            },
        )
        .of(FILE);
    let logo = FileMarks {
        file: ChangedFile::modified("logo.png"),
        marks: Marks::Whole(None),
    };
    let update = question(
        &[location(FILE, SourceSide::New, Some((60, 62)))],
        vec![location(FILE, SourceSide::New, Some((50, 64)))],
        vec![not_relevant(location(FILE, SourceSide::Old, Some((1, 3))))],
        Vec::new(),
    );

    let tally = MarkTally::of(&[queue, logo], Some(&round), Some(&update));

    let files = &tally.files;
    assert_eq!(
        files[0].tally.pending,
        PendingLines {
            reviewed: 12,
            not_relevant: 3
        }
    );
    assert_eq!(files[0].tally.left, 68);
    assert!(files[0].cited && !files[1].cited);
    assert_eq!(tally.change.left, 68);
    let gain = tally.gain.unwrap();
    assert_eq!(
        (
            gain.pending.reviewed,
            gain.pending.not_relevant,
            gain.reopened
        ),
        (12, 3, 0)
    );
    assert_eq!(
        gain.before,
        Share {
            marked: 52,
            changed: 135,
            percent: 38
        }
    );
    assert_eq!(
        gain.after,
        Share {
            marked: 67,
            changed: 135,
            percent: 49
        }
    );
}

#[test]
fn the_question_takes_back_the_marked_lines_its_answer_reopens() {
    let round = round(&[], []);
    let file = Lines::open([], [0, 1])
        .added_by([2, 3], &MarkAuthor::Jev)
        .of(FILE);
    // Line 3 reopens and is marked again: it stays marked.
    let update = question(
        &[],
        vec![
            location(FILE, SourceSide::New, Some((1, 1))),
            location(FILE, SourceSide::New, Some((4, 4))),
        ],
        Vec::new(),
        vec![location(FILE, SourceSide::New, None)],
    );

    let tally = MarkTally::of(&[file], Some(&round), Some(&update));

    let file = tally.files[0].tally;
    assert_eq!((file.pending.reviewed, file.reopened, file.left), (1, 1, 1));
    let gain = tally.gain.unwrap();
    assert_eq!((gain.before.marked, gain.after.marked), (2, 2));
}

#[test]
fn a_binary_file_is_marked_and_left_whole() {
    let round = round(
        &["a1"],
        [(
            "request of a1",
            TurnMarks {
                answer: Some("a1".into()),
                not_relevant: vec![not_relevant(location("logo.png", SourceSide::New, None))],
                ..TurnMarks::default()
            },
        )],
    );
    let binary = |path: &str, author: Option<MarkAuthor>| FileMarks {
        file: ChangedFile::modified(path),
        marks: Marks::Whole(author),
    };
    let files = [
        binary("logo.png", Some(explore("a1"))),
        binary("icon.png", Some(MarkAuthor::Jev)),
        binary("photo.png", None),
        binary("banner.png", None),
        Lines::open([], [0]).of(FILE),
    ];
    let update = question(
        &[location("photo.png", SourceSide::New, None)],
        vec![location("photo.png", SourceSide::New, None)],
        Vec::new(),
        vec![location("icon.png", SourceSide::New, None)],
    );

    let tally = MarkTally::of(&files, Some(&round), Some(&update));

    let whole: Vec<_> = tally.files.iter().map(|file| file.whole).collect();
    assert_eq!(
        whole,
        [
            Some(WholeFile {
                marked_by: Some(Marker::NotRelevant),
                answering: None
            }),
            Some(WholeFile {
                marked_by: Some(Marker::Jev),
                answering: Some(WholeChange::Reopened)
            }),
            Some(WholeFile {
                marked_by: None,
                answering: Some(WholeChange::Reviewed)
            }),
            Some(WholeFile {
                marked_by: None,
                answering: None
            }),
            None,
        ]
    );
    assert!(tally.files[2].cited);
    // A whole file has no lines: the share counts the text lines only.
    assert_eq!(tally.change.changed.lines_added, 1);
    assert_eq!(
        tally.change.share,
        Share {
            marked: 0,
            changed: 1,
            percent: 0
        }
    );
}

#[test]
fn a_file_whose_every_line_is_marked_has_none_left() {
    let done = Lines::open([], [])
        .added_by(0..3, &MarkAuthor::Reviewer)
        .removed_by([0], &MarkAuthor::Jev)
        .of(FILE);

    let file = MarkTally::of(&[done], None, None).files[0].tally;

    assert_eq!(file.left, 0);
    assert_eq!(
        file.share,
        Share {
            marked: 4,
            changed: 4,
            percent: 100
        }
    );
}

#[test]
fn a_share_shows_neither_none_nor_all_while_it_is_partial() {
    let one = Lines::open([], 1..1000)
        .added_by([0], &MarkAuthor::Reviewer)
        .of(FILE);
    let all_but_one = Lines::open([], [999])
        .added_by(0..999, &MarkAuthor::Reviewer)
        .of(FILE);

    assert_eq!(MarkTally::of(&[one], None, None).change.share.percent, 1);
    assert_eq!(
        MarkTally::of(&[all_but_one], None, None)
            .change
            .share
            .percent,
        99
    );
}

#[test]
fn a_file_whose_marks_cannot_be_read_is_left_whole() {
    let mut file = ChangedFile::modified(FILE);
    file.statistics = DiffStatistics {
        lines_added: 4,
        lines_removed: 1,
    };
    let unread = FileMarks {
        file,
        marks: Marks::Unread,
    };

    let tally = MarkTally::of(&[unread], None, None).change;

    assert_eq!(tally.changed.lines_added + tally.changed.lines_removed, 5);
    assert_eq!((tally.left, tally.share.marked), (5, 0));
}

#[test]
fn a_tally_of_counted_files_adds_them_up_as_a_tally_of_marks_does() {
    let file = |path: &str, added: u64, marked: MarkedLines, pending: PendingLines| FileTally {
        path: path.into(),
        tally: Tally::new(
            DiffStatistics {
                lines_added: added,
                lines_removed: 0,
            },
            marked,
            pending,
            0,
        ),
        whole: None,
        cited: false,
    };
    let files = || {
        vec![
            file(
                "src/queue.rs",
                6,
                MarkedLines {
                    answers: 2,
                    ..MarkedLines::default()
                },
                PendingLines {
                    reviewed: 1,
                    not_relevant: 0,
                },
            ),
            file(
                "src/flush.rs",
                4,
                MarkedLines {
                    jev: 1,
                    ..MarkedLines::default()
                },
                PendingLines::default(),
            ),
        ]
    };

    let waiting = MarkTally::of_files(files(), true);

    assert_eq!(waiting.files[0].tally.left, 3);
    assert_eq!(
        waiting.change.share,
        Share {
            marked: 3,
            changed: 10,
            percent: 30
        }
    );
    assert_eq!(waiting.change.left, 6);
    assert_eq!(waiting.gain.map(|gain| gain.after.percent), Some(40));
    assert_eq!(MarkTally::of_files(files(), false).gain, None);
}
