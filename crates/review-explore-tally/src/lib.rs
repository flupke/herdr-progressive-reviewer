//! How much of the change under review the review marks cover, as the Explore page's meter
//! shows it: the changed lines of the change and of each file, the lines marked and by whom,
//! the lines the question the round waits for marks once answered, and the lines left.
//!
//! The marked lines fall in three groups. The reviewer's answers: lines an answer of the
//! round settled. Jev and not relevant: lines Jev dismissed, and lines the round's agent read
//! and found to hold no decision. Everything else: lines the reviewer marked by hand in the
//! Files view or in the diff, and lines an answer or a turn of another round marked. A mark
//! saved with no author was saved before marks named one, when only the reviewer marked lines,
//! by hand: the store reads it as the reviewer's, so it counts as marked by hand.
//!
//! The numbers are plain data for the page: they derive `Serialize` and draw nothing.

mod marks;
mod round;
mod tally;

pub use marks::ChangeMarks;
use marks::{FileMarks, Marks};
use review_explore::{ExploreRound, InterviewUpdate};
use review_hunks::LineCount;
use review_repository::repository::DiffStatistics;
use serde::Serialize;

/// The review marks of the change under review: the change as a whole, each changed file, and
/// what answering the question the round waits for adds.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct MarkTally {
    /// The whole change: "+125 −10 · 4 files", "38% reviewed · 52 of 135 changed lines".
    pub change: Tally,
    /// Each changed file, in the order of the change.
    pub files: Vec<FileTally>,
    /// What answering the question the round waits for does; `None` when no question waits
    /// for an answer, or when the code changed since the round started and an answer marks
    /// nothing.
    pub gain: Option<Gain>,
}

/// One changed file's review marks.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct FileTally {
    /// The repository-relative path.
    pub path: String,
    /// The file's changed lines. A file without lines to mark one by one has none.
    pub tally: Tally,
    /// A file without lines to mark one by one (a binary or another non-text change), which
    /// is marked or left whole; `None` for a file of text lines.
    pub whole: Option<WholeFile>,
    /// Whether the question the round waits for cites the file ("cited here").
    pub cited: bool,
}

/// How the changed lines of a file, or of the whole change, stand.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Tally {
    /// The changed lines, added and removed: "+125 −10".
    pub changed: DiffStatistics,
    /// The changed lines a review mark covers, by who marked them.
    pub marked: MarkedLines,
    /// The open lines that answering the question the round waits for marks.
    pub pending: PendingLines,
    /// The marked lines that answering the question the round waits for reopens.
    pub reopened: u64,
    /// The open lines the question does not mark: changed, less marked, less pending ("Left
    /// to explore"). Unlike the unreviewed lines, they leave out the lines the question marks.
    pub left: u64,
    /// The marked share of the changed lines.
    pub share: Share,
}

/// Marked lines by who marked them, in the meter's three groups: `answers`; `jev` and
/// `not_relevant`; `by_hand` and `other_rounds`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct MarkedLines {
    /// Lines an answer of the round settled.
    pub answers: u64,
    /// Lines Jev dismissed as too insignificant to need the reviewer's attention.
    pub jev: u64,
    /// Lines the round's agent read and found to hold no decision for the reviewer.
    pub not_relevant: u64,
    /// Lines the reviewer marked by hand, and lines whose mark names no author.
    pub by_hand: u64,
    /// Lines an answer or a turn of another round of the review marked.
    pub other_rounds: u64,
}

/// The open lines that answering the question the round waits for marks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PendingLines {
    /// Marked reviewed: the lines the answer settles.
    pub reviewed: u64,
    /// Marked not relevant: the lines the agent found to hold no decision.
    pub not_relevant: u64,
}

/// A share of changed lines that review marks cover: "38% reviewed · 52 of 135 changed lines".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Share {
    pub marked: u64,
    pub changed: u64,
    /// `marked` of `changed` in whole percent, as the reviewer shows a file's reviewed share
    /// ([`LineCount::percent`]): rounded down, but never 0% once a line is marked nor 100%
    /// while one is not; 0 when nothing changed.
    pub percent: u64,
}

/// What answering the question the round waits for does to the whole change: "Answering
/// marks 12 lines reviewed · 3 not relevant", "38% → 49%".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Gain {
    /// Open lines it marks reviewed and not relevant.
    pub pending: PendingLines,
    /// Marked lines it reopens.
    pub reopened: u64,
    /// The marked share now.
    pub before: Share,
    /// The marked share once the answer's marks apply.
    pub after: Share,
}

/// A file without lines to mark one by one, marked or left as a whole.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WholeFile {
    /// Who marked the file, in the meter's groups; `None` while it is left.
    pub marked_by: Option<Marker>,
    /// What answering the question the round waits for does to the file.
    pub answering: Option<WholeChange>,
}

/// Who marked lines, in the groups the meter shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Marker {
    Answer,
    Jev,
    NotRelevant,
    ByHand,
    OtherRound,
}

/// What answering a question does to a file it marks or reopens as a whole.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WholeChange {
    Reviewed,
    NotRelevant,
    Reopened,
}

impl MarkTally {
    /// The tally of `files` while `round` is the round of the review, if any, and the round
    /// waits for an answer to the question that the agent's turn `question` posted, if any.
    pub(crate) fn of(
        files: &[FileMarks],
        round: Option<&ExploreRound>,
        question: Option<&InterviewUpdate>,
    ) -> Self {
        let round = round::RoundMarks::new(round);
        let answering = question.map(round::Answering::new);
        let files: Vec<FileTally> = files
            .iter()
            .map(|file| file.tally(&round, answering.as_ref()))
            .collect();
        let change = files.iter().map(|file| file.tally).sum();
        Self {
            gain: answering.map(|_| Gain::of(&change)),
            change,
            files,
        }
    }
}

impl Gain {
    fn of(change: &Tally) -> Self {
        let marked = (change.share.marked + change.pending.total()).saturating_sub(change.reopened);
        Self {
            pending: change.pending,
            reopened: change.reopened,
            before: change.share,
            after: Share::of(marked, change.share.changed),
        }
    }
}

impl MarkedLines {
    /// Every marked line.
    pub(crate) fn total(&self) -> u64 {
        self.answers + self.jev + self.not_relevant + self.by_hand + self.other_rounds
    }

    fn count(&mut self, marker: Marker) {
        *match marker {
            Marker::Answer => &mut self.answers,
            Marker::Jev => &mut self.jev,
            Marker::NotRelevant => &mut self.not_relevant,
            Marker::ByHand => &mut self.by_hand,
            Marker::OtherRound => &mut self.other_rounds,
        } += 1;
    }
}

impl PendingLines {
    /// Every line the answer marks.
    pub(crate) fn total(&self) -> u64 {
        self.reviewed + self.not_relevant
    }
}

impl Share {
    fn of(marked: u64, changed: u64) -> Self {
        let lines = |count: u64| u32::try_from(count).unwrap_or(u32::MAX);
        let count = LineCount {
            reviewed: lines(marked),
            total: lines(changed),
        };
        Self {
            marked,
            changed,
            percent: u64::from(count.percent()),
        }
    }
}

/// A count of lines, as the numbers carry it.
fn count(lines: usize) -> u64 {
    u64::try_from(lines).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
