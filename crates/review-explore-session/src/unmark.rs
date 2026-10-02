//! Giving back the review marks of a cancelled turn: the lines its answer's
//! author still holds reopen, and the lines it reopened return to whoever
//! had marked them.

use std::collections::BTreeMap;

use review_explore::{CodeLocation, ExploreRound, ReopenedLines, TurnMarks};
use review_hunks::LineSelection;
use review_repository::repository::{ChangedFile, PollResult, Snapshot};
use review_state::{FileLines, ReviewTracker};
use review_types::MarkAuthor;

use super::FileRequest;
use crate::ExploreSession;

/// The marks a cancelled turn changed on one file.
struct FileUnmark<'a> {
    file: &'a ChangedFile,
    reopened: Vec<&'a ReopenedLines>,
}

impl<'a> FileUnmark<'a> {
    /// The files `marks` changed, in snapshot order.
    fn group(snapshot: &'a Snapshot, marks: &'a TurnMarks) -> Vec<Self> {
        snapshot
            .files
            .iter()
            .filter(|file| {
                marks.reviewed.iter().any(|location| location.names(file))
                    || marks
                        .reopened
                        .iter()
                        .any(|reopened| reopened.location.names(file))
            })
            .map(|file| Self {
                file,
                reopened: marks
                    .reopened
                    .iter()
                    .filter(|reopened| reopened.location.names(file))
                    .collect(),
            })
            .collect()
    }

    /// Reopen what `author` holds, then give back what it reopened when the
    /// code still has the lines it named; whether lines stayed open.
    fn apply(
        &self,
        tracker: &ReviewTracker,
        snapshot: &Snapshot,
        author: &MarkAuthor,
        unchanged: bool,
    ) -> eyre::Result<bool> {
        let before = tracker.lines(snapshot, self.file)?;
        if before == FileLines::default() {
            return self.apply_whole(tracker, snapshot, author, unchanged);
        }
        let held = held_by(&before, author);
        if !held.is_empty() {
            tracker.reopen_lines(snapshot, self.file, &held)?;
        }
        if !unchanged {
            return Ok(!self.reopened.is_empty());
        }
        let mut by_author: BTreeMap<&MarkAuthor, Vec<&CodeLocation>> = BTreeMap::new();
        for reopened in &self.reopened {
            by_author
                .entry(&reopened.author)
                .or_default()
                .push(&reopened.location);
        }
        for (previous, locations) in by_author {
            let open = tracker.lines(snapshot, self.file)?.open_selection();
            let named = FileRequest::selection(&locations, || open.clone());
            let restore = named.difference(&named.difference(&open));
            if !restore.is_empty() {
                tracker.accept_lines(snapshot, self.file, &restore, previous)?;
            }
        }
        Ok(false)
    }

    /// A file without lines to mark one by one is held or reopened whole.
    fn apply_whole(
        &self,
        tracker: &ReviewTracker,
        snapshot: &Snapshot,
        author: &MarkAuthor,
        unchanged: bool,
    ) -> eyre::Result<bool> {
        let held = tracker.whole_file_author(snapshot, self.file)?;
        if held.as_ref() == Some(author) {
            tracker.unreview(snapshot, self.file)?;
        }
        let previous = self.reopened.first().map(|reopened| &reopened.author);
        match previous {
            Some(previous) if unchanged => {
                if held.is_none() || held.as_ref() == Some(author) {
                    tracker.mark(snapshot, self.file, previous)?;
                }
                Ok(false)
            }
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }
}

/// The reviewed lines `author` marked.
fn held_by(lines: &FileLines, author: &MarkAuthor) -> LineSelection {
    let mine = |side: &BTreeMap<u32, MarkAuthor>| {
        side.iter()
            .filter(|(_, marked_by)| *marked_by == author)
            .map(|(line, _)| *line)
            .collect()
    };
    LineSelection {
        removed: mine(&lines.reviewed.removed),
        added: mine(&lines.reviewed.added),
    }
}

impl ExploreSession {
    /// Give back the review marks a cancelled turn changed, file by file,
    /// failing when any file failed so the cancel can be tried again. Lines
    /// it reopened stay open when the code changed since the round started;
    /// the returned problem says so.
    pub(crate) fn unmark(
        &self,
        round: &ExploreRound,
        marks: &TurnMarks,
    ) -> eyre::Result<Option<String>> {
        let PollResult::Complete(snapshot) = self.repository.poll()? else {
            eyre::bail!("the repository is not ready; try again");
        };
        let unchanged = round.exploration.comparison.checkpoint.matches(
            snapshot.identity.review_unit(),
            snapshot.identity.snapshot_id(),
        );
        let author = MarkAuthor::Explore {
            answer: marks.answer.clone(),
        };
        let mut kept_open = false;
        let mut failed = None;
        for file in FileUnmark::group(&snapshot, marks) {
            let path = file.file.review_path().display();
            match file.apply(&self.tracker, &snapshot, &author, unchanged) {
                Ok(open) => kept_open |= open,
                Err(error) => {
                    failed.get_or_insert(format!("{path}: {error}"));
                }
            }
            let _ = self.events.send(ui_events::ReviewStateSaved {
                review_unit: snapshot.identity.review_unit().clone(),
                path,
                result: self.tracker.status(&snapshot, file.file).map_err(|_| ()),
            });
        }
        if let Some(failed) = failed {
            eyre::bail!("review marks were not given back: {failed}");
        }
        Ok(kept_open.then(|| {
            "the code changed since this round started; the lines its turn reopened stay open"
                .to_owned()
        }))
    }
}
