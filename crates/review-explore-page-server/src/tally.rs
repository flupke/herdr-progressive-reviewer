//! How much of the change the review marks cover, as the standalone server tells it. The review
//! tool counts the marks it saved, line by line (`review_explore_tally`); the standalone server
//! saves none, so it counts lines: the lines Jev marked when the round started, the lines each
//! answer of the round marked, the lines the reviewer marked by hand meanwhile, and
//! the lines the question that waits marks once answered. A mark covers as many lines of a
//! file as its location spans, and never more than the file has left; the totals follow the
//! review tool's rules (`MarkTally::of_files`).

use review_explore::{CodeLocation, NotRelevantMark, Question};
use review_explore_page::QuestionMarks;
use review_explore_tally::{FileTally, MarkTally, MarkedLines, PendingLines, Tally};
use review_repository::repository::RepoPath;

use crate::changed_source::{ChangedSource, FixedChange};

/// Lines of a file of the change that Jev marks when a round starts.
pub(crate) struct JevMark {
    pub(crate) path: &'static str,
    pub(crate) lines: u64,
}

/// What a session's marks are, as it counts them.
pub(crate) struct SessionMarks<'a> {
    pub(crate) change: &'a FixedChange,
    /// The lines Jev marks when a round starts, which count only while a round runs.
    pub(crate) jev: &'a [JevMark],
    /// Whether a round runs.
    pub(crate) running: bool,
    /// What each answer of the round marked, in order.
    pub(crate) answered: &'a [QuestionMarks],
    /// The lines the reviewer marked by hand since, in the change's first file.
    pub(crate) by_hand: u64,
    /// The question that waits for an answer, with what its answer marks.
    pub(crate) waiting: Option<(&'a Question, &'a QuestionMarks)>,
}

impl SessionMarks<'_> {
    pub(crate) fn tally(&self) -> MarkTally {
        let files = self
            .change
            .0
            .iter()
            .enumerate()
            .map(|(index, source)| self.file(source, index == 0))
            .collect();
        MarkTally::of_files(files, self.waiting.is_some())
    }

    /// The tally of `source`, the change's first file when `first`.
    fn file(&self, source: &ChangedSource, first: bool) -> FileTally {
        let path = RepoPath::from_bytes(source.path);
        let changed = source.statistics();
        let mut lines = Lines::new(changed.lines());
        let mut marked = MarkedLines::default();
        if self.running {
            for jev in self.jev.iter().filter(|jev| jev.path == source.path) {
                marked.jev += lines.take(jev.lines);
            }
        }
        for answer in self.answered {
            marked.answers += lines.take(lines.within(&path, &answer.reviewed));
            marked.not_relevant += lines.take(lines.within(&path, not_relevant(answer)));
        }
        if first {
            marked.by_hand += lines.take(self.by_hand);
        }
        let mut pending = PendingLines::default();
        if let Some((_, marks)) = self.waiting {
            pending.reviewed = lines.take(lines.within(&path, &marks.reviewed));
            pending.not_relevant = lines.take(lines.within(&path, not_relevant(marks)));
        }
        FileTally {
            path: source.path.into(),
            tally: Tally::new(changed, marked, pending, 0),
            whole: None,
            cited: self.waiting.is_some_and(|(question, _)| {
                question
                    .evidence
                    .iter()
                    .any(|evidence| evidence.location.path == path)
            }),
        }
    }
}

/// The changed lines of a file that no mark took yet.
struct Lines {
    total: u64,
    left: u64,
}

impl Lines {
    fn new(total: u64) -> Self {
        Self { total, left: total }
    }

    /// Takes up to `lines` of the lines left, and returns how many it took.
    fn take(&mut self, lines: u64) -> u64 {
        let taken = lines.min(self.left);
        self.left -= taken;
        taken
    }

    /// The lines of the file at `path` that `locations` span: a whole file spans every line.
    fn within<'a>(
        &self,
        path: &RepoPath,
        locations: impl IntoIterator<Item = &'a CodeLocation>,
    ) -> u64 {
        locations
            .into_iter()
            .filter(|location| location.path == *path)
            .map(|location| {
                location
                    .lines
                    .as_ref()
                    .map_or(self.total, |lines| u64::from(lines.count()))
            })
            .sum()
    }
}

fn not_relevant(marks: &QuestionMarks) -> impl Iterator<Item = &CodeLocation> {
    NotRelevantMark::locations(&marks.not_relevant)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::Rich;
    use crate::round_data::RoundData;

    fn marks<'a>(
        answered: &'a [QuestionMarks],
        waiting: Option<(&'a Question, &'a QuestionMarks)>,
    ) -> SessionMarks<'a> {
        SessionMarks {
            change: Rich.change(),
            jev: Rich.jev_marks(),
            running: true,
            answered,
            by_hand: 0,
            waiting,
        }
    }

    #[test]
    fn an_answer_marks_the_lines_its_question_said_it_would() {
        let (question, first) = Rich.question(1);

        let waiting = marks(&[], Some((&question, &first))).tally();
        let answered = marks(std::slice::from_ref(&first), None).tally();

        assert_eq!(
            waiting.change.pending,
            PendingLines {
                reviewed: 29,
                not_relevant: 5
            }
        );
        assert!(waiting.files[0].cited);
        assert_eq!(
            answered.change.marked,
            MarkedLines {
                answers: 29,
                jev: 10,
                not_relevant: 5,
                ..MarkedLines::default()
            }
        );
        assert_eq!(
            waiting.gain.map(|gain| gain.after),
            Some(answered.change.share)
        );
        assert_eq!(answered.gain, None);
    }

    #[test]
    fn jev_marks_the_change_only_once_a_round_runs() {
        let no_round = SessionMarks {
            running: false,
            ..marks(&[], None)
        }
        .tally();

        assert_eq!(no_round.change.marked, MarkedLines::default());
        assert_eq!(marks(&[], None).tally().change.marked.jev, 10);
    }

    #[test]
    fn a_mark_takes_no_more_lines_than_its_file_has_left() {
        let tally = SessionMarks {
            by_hand: 1000,
            ..marks(&[], None)
        }
        .tally();

        assert_eq!(tally.files[0].tally.left, 0);
        assert_eq!(
            tally.files[0].tally.share.marked,
            tally.files[0].tally.changed.lines()
        );
    }
}
