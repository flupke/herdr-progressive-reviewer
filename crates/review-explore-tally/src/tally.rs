//! Counting one file's lines, and adding files up.

use review_explore::SourceSide;
use review_repository::repository::DiffStatistics;
use review_state::FileLines;
use review_types::MarkAuthor;

use crate::round::{AnswerEffect, Answering, FileMarkers, Line, RoundMarks};
use crate::{
    FileMarks, FileTally, MarkedLines, Marks, PendingLines, Share, Tally, WholeFile, count,
};

impl FileMarks {
    /// The file's tally in the round `round`, while answering `answering` waits.
    pub(crate) fn tally(
        &self,
        round: &RoundMarks<'_>,
        answering: Option<&Answering<'_>>,
    ) -> FileTally {
        let markers = round.in_file(&self.file);
        let (tally, whole) = match &self.marks {
            Marks::Lines(lines) => (self.lines(lines, &markers, answering), None),
            Marks::Whole(author) => (
                Tally::default(),
                Some(self.whole(author.as_ref(), &markers, answering)),
            ),
            Marks::Unread => (Tally::left(self.file.statistics), None),
        };
        FileTally {
            path: self.file.review_path().display(),
            tally,
            whole,
            cited: answering.is_some_and(|answering| answering.cites(&self.file)),
        }
    }

    fn lines(
        &self,
        lines: &FileLines,
        markers: &FileMarkers<'_>,
        answering: Option<&Answering<'_>>,
    ) -> Tally {
        let open = lines.open_selection();
        let reviewed = &lines.reviewed;
        let mut marked = MarkedLines::default();
        for (side, authors) in [
            (SourceSide::Old, &reviewed.removed),
            (SourceSide::New, &reviewed.added),
        ] {
            for (line, author) in authors {
                marked.count(markers.marker(author, Line::One(side, *line)));
            }
        }
        let effect = answering.map_or_else(AnswerEffect::default, |answering| {
            answering.on_lines(&self.file, &open, reviewed)
        });
        let changed = DiffStatistics {
            lines_added: count(open.added.len() + reviewed.added.len()),
            lines_removed: count(open.removed.len() + reviewed.removed.len()),
        };
        Tally::new(changed, marked, effect.pending, effect.reopened)
    }

    fn whole(
        &self,
        author: Option<&MarkAuthor>,
        markers: &FileMarkers<'_>,
        answering: Option<&Answering<'_>>,
    ) -> WholeFile {
        WholeFile {
            marked_by: author.map(|author| markers.marker(author, Line::Whole)),
            answering: answering
                .and_then(|answering| answering.on_whole(&self.file, author.is_some())),
        }
    }
}

impl Tally {
    /// The tally of `changed` lines of which `marked` are marked, of which answering the
    /// question the round waits for marks the open lines `pending` and reopens `reopened`
    /// marked lines.
    pub fn new(
        changed: DiffStatistics,
        marked: MarkedLines,
        pending: PendingLines,
        reopened: u64,
    ) -> Self {
        let total = changed.lines();
        let share = Share::of(marked.total(), total);
        Self {
            changed,
            marked,
            pending,
            reopened,
            left: total.saturating_sub(share.marked + pending.total()),
            share,
        }
    }

    /// `changed` lines, none of them marked.
    fn left(changed: DiffStatistics) -> Self {
        Self::new(changed, MarkedLines::default(), PendingLines::default(), 0)
    }
}

impl std::iter::Sum for Tally {
    fn sum<I: Iterator<Item = Self>>(tallies: I) -> Self {
        let mut changed = DiffStatistics::default();
        let mut marked = MarkedLines::default();
        let mut effect = AnswerEffect::default();
        for tally in tallies {
            changed.lines_added += tally.changed.lines_added;
            changed.lines_removed += tally.changed.lines_removed;
            marked += tally.marked;
            effect.pending += tally.pending;
            effect.reopened += tally.reopened;
        }
        Self::new(changed, marked, effect.pending, effect.reopened)
    }
}

impl std::ops::AddAssign for MarkedLines {
    fn add_assign(&mut self, other: Self) {
        self.answers += other.answers;
        self.jev += other.jev;
        self.not_relevant += other.not_relevant;
        self.by_hand += other.by_hand;
        self.other_rounds += other.other_rounds;
    }
}

impl std::ops::AddAssign for PendingLines {
    fn add_assign(&mut self, other: Self) {
        self.reviewed += other.reviewed;
        self.not_relevant += other.not_relevant;
    }
}
