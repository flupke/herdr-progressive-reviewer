//! The unreviewed lines each prompt lists, read from the review marks.

use review_explore::SourceSide;
use review_explore_runner::{
    Unreviewed, UnreviewedFile, UnreviewedLines, UnreviewedRange, UnreviewedStatus,
};
use review_repository::repository::{ChangedFile, PollResult, Snapshot};
use review_source::SourceLineRange;
use review_state::{FileLines, ReviewStatus};

use crate::ExploreSession;
use crate::marks::runs;

impl ExploreSession {
    /// What no review mark covers in the current snapshot, or why it cannot
    /// be read. A kickoff also says what Jev marked before the pass.
    pub(crate) fn unreviewed(&self, kickoff: bool) -> Unreviewed {
        let jev = self.state.jev.clone().filter(|_| kickoff);
        let unavailable = |why: String| Unreviewed {
            jev: jev.clone(),
            status: UnreviewedStatus::Unavailable(why),
            ..Unreviewed::default()
        };
        let snapshot = match self.repository.poll() {
            Ok(PollResult::Complete(snapshot)) => snapshot,
            Ok(_) => return unavailable("the repository is still loading".into()),
            Err(error) => return unavailable(error.to_string()),
        };
        let states = match self.tracker.statuses(&snapshot) {
            Ok(states) => states,
            Err(error) => return unavailable(error.to_string()),
        };
        let moved = self.state.comparison.as_ref().is_some_and(|comparison| {
            !comparison.checkpoint.matches(
                snapshot.identity.review_unit(),
                snapshot.identity.snapshot_id(),
            )
        });
        let files = snapshot
            .files
            .iter()
            .zip(states)
            .filter(|(_, state)| state.status != ReviewStatus::Reviewed)
            .map(|(file, state)| UnreviewedFile {
                path: file.review_path().clone(),
                lines: if state.status == ReviewStatus::Unreviewed {
                    whole(file)
                } else {
                    self.open_lines(&snapshot, file)
                },
            })
            .collect();
        Unreviewed {
            paths: snapshot
                .files
                .iter()
                .map(|file| file.review_path().clone())
                .collect(),
            files,
            jev,
            status: UnreviewedStatus::Listed {
                notice: moved.then(|| {
                    "the code changed since this pass started: these lines are numbered like \
                     the current code, and review marks are not applied until a new pass starts"
                        .into()
                }),
            },
        }
    }

    /// The open lines of a partly reviewed file. A file whose lines cannot
    /// be read, or whose open hunks only delete reviewed lines (they have
    /// no number to name), is listed whole.
    fn open_lines(&self, snapshot: &Snapshot, file: &ChangedFile) -> UnreviewedLines {
        match self.tracker.lines(snapshot, file) {
            Ok(lines) if !lines.open_selection().is_empty() => {
                UnreviewedLines::Ranges(ranges(&lines))
            }
            _ => whole(file),
        }
    }
}

fn whole(file: &ChangedFile) -> UnreviewedLines {
    UnreviewedLines::Whole {
        changed: file.statistics.lines_added + file.statistics.lines_removed,
    }
}

/// One-based runs of each open hunk's lines, removed lines first.
fn ranges(lines: &FileLines) -> Vec<UnreviewedRange> {
    let mut ranges = Vec::new();
    for open in &lines.open {
        for (side, lines) in [
            (SourceSide::Old, &open.lines.removed),
            (SourceSide::New, &open.lines.added),
        ] {
            for (lines, ()) in runs(lines.iter().map(|line| (*line, ()))) {
                ranges.push(UnreviewedRange {
                    side,
                    lines: SourceLineRange::from_zero_based(lines),
                    since_review: open.since_review,
                });
            }
        }
    }
    ranges
}
