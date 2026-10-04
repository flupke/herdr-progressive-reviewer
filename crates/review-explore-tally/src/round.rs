//! Who in the round marked each line, and what the question the round waits for marks once
//! answered.

use review_explore::{
    ExploreRound, InterviewUpdate, NamedLines, NotRelevantMark, SourceSide, TurnMarks,
};
use review_hunks::{LineSelection, ReviewedLines};
use review_repository::repository::ChangedFile;
use review_types::MarkAuthor;

use crate::{Marker, PendingLines, WholeChange, count};

/// The round of the review, if any, which tells its own marks from those of other rounds.
pub(crate) struct RoundMarks<'a>(Option<&'a ExploreRound>);

/// The round's marks on one file: the lines each answer found not relevant.
pub(crate) struct FileMarkers<'a> {
    round: &'a RoundMarks<'a>,
    not_relevant: Vec<(&'a str, NamedLines)>,
}

/// A changed line, or the whole file for a file without lines.
#[derive(Clone, Copy)]
pub(crate) enum Line {
    One(SourceSide, u32),
    Whole,
}

impl<'a> RoundMarks<'a> {
    pub(crate) fn new(round: Option<&'a ExploreRound>) -> Self {
        Self(round)
    }

    /// The round's marks on `file`.
    pub(crate) fn in_file(&'a self, file: &ChangedFile) -> FileMarkers<'a> {
        let marks = self
            .0
            .map(|round| round.marks.values())
            .into_iter()
            .flatten();
        FileMarkers {
            round: self,
            not_relevant: marks
                .filter_map(|marks: &TurnMarks| {
                    let answer = marks.answer.as_deref()?;
                    let named =
                        NamedLines::in_file(file, NotRelevantMark::locations(&marks.not_relevant))?;
                    Some((answer, named))
                })
                .collect(),
        }
    }

    fn has_answer(&self, answer: &str) -> bool {
        self.0.is_some_and(|round| {
            round
                .exploration
                .answers
                .iter()
                .any(|given| given.id == answer)
        })
    }

    fn has_turn(&self, request: &str) -> bool {
        self.0.is_some_and(|round| {
            round.marks.contains_key(request) || round.turns.contains_key(request)
        })
    }
}

impl FileMarkers<'_> {
    /// Who, in the meter's groups, marked `line` as `author`.
    pub(crate) fn marker(&self, author: &MarkAuthor, line: Line) -> Marker {
        match author {
            MarkAuthor::Reviewer => Marker::ByHand,
            MarkAuthor::Jev => Marker::Jev,
            // A turn that follows no answer marks only lines it found not relevant: the
            // round refuses its reviewed and reopened lines.
            MarkAuthor::ExploreRead { request } if self.round.has_turn(request) => {
                Marker::NotRelevant
            }
            MarkAuthor::Explore { answer } if self.round.has_answer(answer) => {
                if self.found_not_relevant(answer, line) {
                    Marker::NotRelevant
                } else {
                    Marker::Answer
                }
            }
            MarkAuthor::ExploreRead { .. } | MarkAuthor::Explore { .. } => Marker::OtherRound,
        }
    }

    /// Whether the turn `answer` applied marked `line` not relevant.
    fn found_not_relevant(&self, answer: &str, line: Line) -> bool {
        self.not_relevant
            .iter()
            .any(|(applied, named)| *applied == answer && line.in_lines(named))
    }
}

impl Line {
    fn in_lines(self, named: &NamedLines) -> bool {
        match self {
            Self::One(side, line) => named.contains(side, line),
            Self::Whole => true,
        }
    }
}

/// The marks that answering the question the round waits for applies, as the agent's turn
/// that posted it asked.
pub(crate) struct Answering<'a>(&'a InterviewUpdate);

/// The open lines answering marks, and the marked lines it reopens.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AnswerEffect {
    pub(crate) pending: PendingLines,
    pub(crate) reopened: u64,
}

/// The lines answering marks and reopens on one file.
struct FileAnswer {
    reviewed: Option<NamedLines>,
    not_relevant: Option<NamedLines>,
    reopened: Option<NamedLines>,
}

impl<'a> Answering<'a> {
    pub(crate) fn new(question: &'a InterviewUpdate) -> Self {
        Self(question)
    }

    /// Whether the question cites `file`.
    pub(crate) fn cites(&self, file: &ChangedFile) -> bool {
        self.0.next.as_ref().is_some_and(|question| {
            question
                .evidence
                .iter()
                .any(|evidence| evidence.location.names(file))
        })
    }

    fn in_file(&self, file: &ChangedFile) -> FileAnswer {
        FileAnswer {
            reviewed: NamedLines::in_file(file, &self.0.reviewed),
            not_relevant: NamedLines::in_file(
                file,
                NotRelevantMark::locations(&self.0.not_relevant),
            ),
            reopened: NamedLines::in_file(file, &self.0.reopened),
        }
    }

    /// The open lines of `file` the answer marks, and how many of its `reviewed` lines it
    /// reopens. Marks apply to open lines only; a reviewed line it reopens and marks again
    /// stays marked.
    pub(crate) fn on_lines(
        &self,
        file: &ChangedFile,
        open: &LineSelection,
        reviewed: &ReviewedLines,
    ) -> AnswerEffect {
        let answer = self.in_file(file);
        let mut pending = PendingLines::default();
        for line in lines(open.removed.iter().copied(), open.added.iter().copied()) {
            if answer.marks_reviewed(line) {
                pending.reviewed += 1;
            } else if FileAnswer::names(answer.not_relevant.as_ref(), line) {
                pending.not_relevant += 1;
            }
        }
        let reopened = lines(
            reviewed.removed.keys().copied(),
            reviewed.added.keys().copied(),
        )
        .filter(|line| answer.reopens(*line))
        .count();
        AnswerEffect {
            pending,
            reopened: count(reopened),
        }
    }

    /// What the answer does to `file`, which is marked or left whole.
    pub(crate) fn on_whole(&self, file: &ChangedFile, marked: bool) -> Option<WholeChange> {
        let answer = self.in_file(file);
        if marked {
            return answer.reopens(Line::Whole).then_some(WholeChange::Reopened);
        }
        if answer.reviewed.is_some() {
            Some(WholeChange::Reviewed)
        } else {
            answer
                .not_relevant
                .is_some()
                .then_some(WholeChange::NotRelevant)
        }
    }
}

impl FileAnswer {
    fn names(named: Option<&NamedLines>, line: Line) -> bool {
        named.is_some_and(|named| line.in_lines(named))
    }

    fn marks_reviewed(&self, line: Line) -> bool {
        Self::names(self.reviewed.as_ref(), line)
    }

    /// Whether the answer reopens the marked `line` and leaves it open.
    fn reopens(&self, line: Line) -> bool {
        Self::names(self.reopened.as_ref(), line)
            && !self.marks_reviewed(line)
            && !Self::names(self.not_relevant.as_ref(), line)
    }
}

/// The lines `removed` and `added` number, each on its side.
fn lines(
    removed: impl Iterator<Item = u32>,
    added: impl Iterator<Item = u32>,
) -> impl Iterator<Item = Line> {
    removed
        .map(|line| Line::One(SourceSide::Old, line))
        .chain(added.map(|line| Line::One(SourceSide::New, line)))
}
