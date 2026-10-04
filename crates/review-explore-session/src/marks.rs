//! Review marks an agent turn asks for: the lines an answer settled, the
//! reviewed lines it made matter again, and the lines the agent read and
//! found to hold no decision. A question's marks wait for its answer; a
//! conclusion's apply when it is accepted.

use std::ops::Range;

use review_explore::{
    CodeLocation, ExploreRound, InterviewUpdate, NamedLines, NotRelevantMark, ReopenedLines,
    SourceSide, TurnMarks, TurnRequest,
};
use review_hunks::{LineSelection, ReviewedLines};
use review_repository::repository::{ChangedFile, PollResult, Snapshot};
use review_state::{FileLines, ReviewTracker};
use review_types::MarkAuthor;

use crate::ExploreSession;

/// The marks a turn asks for on one changed file.
struct FileRequest<'a> {
    file: &'a ChangedFile,
    reviewed: Vec<&'a CodeLocation>,
    reopened: Vec<&'a CodeLocation>,
}

/// One step of a turn's marks: first what an answer settled and reopened, file by file, then
/// each line range the agent found not relevant, with its mark.
struct MarkStep<'a> {
    request: FileRequest<'a>,
    not_relevant: Option<&'a NotRelevantMark>,
}

impl<'a> MarkStep<'a> {
    fn of(snapshot: &'a Snapshot, update: &'a InterviewUpdate) -> Vec<Self> {
        let settled = FileRequest::group(snapshot, &update.reviewed, &update.reopened)
            .into_iter()
            .map(|request| Self {
                request,
                not_relevant: None,
            });
        let not_relevant = update.not_relevant.iter().flat_map(|mark| {
            FileRequest::group(snapshot, std::slice::from_ref(&mark.location), &[])
                .into_iter()
                .map(move |request| Self {
                    request,
                    not_relevant: Some(mark),
                })
        });
        settled.chain(not_relevant).collect()
    }
}

/// Applies the marks `update` asks for to `tracker`, which holds a copy of the review marks,
/// as `author`, and tells nobody: the review marks as they will be once the reviewer answers.
pub(crate) fn mark_copy(
    tracker: &ReviewTracker,
    snapshot: &Snapshot,
    update: &InterviewUpdate,
    author: &MarkAuthor,
) {
    for step in MarkStep::of(snapshot, update) {
        let _ = step.request.apply(tracker, snapshot, author);
    }
}

/// The marks a turn changed on one file.
#[derive(Default)]
struct FileChange {
    reviewed: Vec<CodeLocation>,
    reopened: Vec<ReopenedLines>,
}

impl FileChange {
    /// One-based locations of zero-based lines, a range per run on each side.
    fn locations(file: &ChangedFile, selection: &LineSelection) -> Vec<CodeLocation> {
        let mut locations = Vec::new();
        for (side, lines) in [
            (SourceSide::Old, &selection.removed),
            (SourceSide::New, &selection.added),
        ] {
            for run in runs(lines.iter().map(|line| (*line, ()))) {
                locations.push(CodeLocation::on(file, side, run.0));
            }
        }
        locations
    }

    /// The reviewed lines `after` no longer has, or has from another author,
    /// by who had marked them.
    fn reopened(
        file: &ChangedFile,
        before: &ReviewedLines,
        after: &ReviewedLines,
    ) -> Vec<ReopenedLines> {
        let mut reopened = Vec::new();
        for (side, before, after) in [
            (SourceSide::Old, &before.removed, &after.removed),
            (SourceSide::New, &before.added, &after.added),
        ] {
            // Gone, or now someone else's.
            let gone = before
                .iter()
                .filter(|(line, author)| after.get(*line) != Some(*author))
                .map(|(line, author)| (*line, author.clone()));
            for (lines, author) in runs(gone) {
                reopened.push(ReopenedLines {
                    location: CodeLocation::on(file, side, lines),
                    author,
                });
            }
        }
        reopened
    }

    /// What changed between two states of a file's lines. A reviewed line
    /// `author` took over from another author counts as reopened from that
    /// author and marked again, so cancelling the turn can give it back.
    fn between(
        file: &ChangedFile,
        before: &FileLines,
        after: &FileLines,
        author: &MarkAuthor,
    ) -> Self {
        let mut marked = before.open_selection().difference(&after.open_selection());
        for (before, after, marked) in [
            (
                &before.reviewed.removed,
                &after.reviewed.removed,
                &mut marked.removed,
            ),
            (
                &before.reviewed.added,
                &after.reviewed.added,
                &mut marked.added,
            ),
        ] {
            marked.extend(after.iter().filter_map(|(line, now)| {
                (now == author && before.get(line).is_some_and(|then| then != author))
                    .then_some(*line)
            }));
        }
        Self {
            reviewed: Self::locations(file, &marked),
            reopened: Self::reopened(file, &before.reviewed, &after.reviewed),
        }
    }
}

impl ExploreSession {
    /// Apply the marks of the question `request` answers, now that the
    /// reviewer answered it: review progress moves on the reviewer's action.
    /// The round with the record of what changed, or `None` when there is
    /// nothing to apply or the answer already applied it.
    pub(crate) fn apply_answered_marks(
        &mut self,
        request: &TurnRequest,
        round: &ExploreRound,
    ) -> Option<ExploreRound> {
        let answer = request.answer.as_ref()?;
        if round.marks.contains_key(&answer.in_reply_to) {
            return None;
        }
        let update = &round
            .exploration
            .conversation
            .iter()
            .find(|turn| turn.update.request == answer.in_reply_to)?
            .update;
        // A conclusion's marks were applied when it was accepted.
        update.next.as_ref()?;
        self.apply_marks(update, round, Some(answer.id.clone()))
    }

    /// Apply the marks of an accepted conclusion at once, as the answer it
    /// follows.
    pub(crate) fn apply_conclusion_marks(
        &mut self,
        update: &InterviewUpdate,
        round: &ExploreRound,
    ) -> Option<ExploreRound> {
        update.conclusion.as_ref()?;
        let answer = round
            .turns
            .get(&update.request)?
            .request
            .answer
            .as_ref()
            .map(|answer| answer.id.clone());
        self.apply_marks(update, round, answer)
    }

    /// Apply the marks a turn asked for as `answer`, and record what changed;
    /// the round with that record, or `None` when the turn asked for none.
    fn apply_marks(
        &mut self,
        update: &InterviewUpdate,
        round: &ExploreRound,
        answer: Option<String>,
    ) -> Option<ExploreRound> {
        if !update.requests_marks() {
            return None;
        }
        let marks = match self.marked_snapshot(round) {
            Ok(snapshot) => {
                let marks = self.mark(&snapshot, update, answer);
                self.read_marks(&snapshot);
                marks
            }
            Err(problem) => TurnMarks {
                answer,
                problem: Some(problem),
                ..TurnMarks::default()
            },
        };
        if let Some(problem) = &marks.problem {
            let _ = self.events.send(ui_events::ToastRequested {
                text: format!("Explore review marks: {problem}"),
                kind: toasts::ToastKind::Error,
            });
        }
        let checkpoint = &round.exploration.comparison.checkpoint;
        match self.rounds.update(
            &checkpoint.review_unit,
            &round.exploration.instance,
            |round| {
                round.marks.insert(update.request.clone(), marks);
                Ok(())
            },
        ) {
            Ok(((), round)) => Some(round),
            Err(error) => {
                let _ = self
                    .events
                    .send(ui_events::ExploreStorageFailed(error.to_string()));
                None
            }
        }
    }

    /// The current snapshot, when it is still the round's checkpoint.
    fn marked_snapshot(&self, round: &ExploreRound) -> Result<Snapshot, String> {
        let PollResult::Complete(snapshot) =
            self.repository.poll().map_err(|error| error.to_string())?
        else {
            return Err("the repository is not ready; no lines were marked".into());
        };
        if !round.is_at(&snapshot.identity) {
            return Err("the code changed since this round started; no lines were marked".into());
        }
        Ok(snapshot)
    }

    /// Apply a turn's marks: first what an answer settled and reopened, then
    /// the lines the agent found not relevant, each with its mark's reason.
    /// All carry the author of the answer that applies them, so cancelling it
    /// gives them back; a conclusion that opens the round follows no answer
    /// and its marks carry the turn's own.
    fn mark(
        &self,
        snapshot: &Snapshot,
        update: &InterviewUpdate,
        answer: Option<String>,
    ) -> TurnMarks {
        let author = match &answer {
            Some(answer) => MarkAuthor::Explore {
                answer: answer.clone(),
            },
            None => MarkAuthor::ExploreRead {
                request: update.request.clone(),
            },
        };
        let mut marks = TurnMarks {
            answer,
            ..TurnMarks::default()
        };
        for step in MarkStep::of(snapshot, update) {
            let change = self.apply(snapshot, &step.request, &author, &mut marks);
            match step.not_relevant {
                // One mark at a time, so the lines it marks keep its reason and test.
                Some(mark) => {
                    marks
                        .not_relevant
                        .extend(change.reviewed.into_iter().map(|location| NotRelevantMark {
                            location,
                            ..mark.clone()
                        }));
                }
                None => marks.reviewed.extend(change.reviewed),
            }
        }
        marks
    }

    /// Apply one file's marks and tell the UI its new review state. Lines
    /// taken over from another author are recorded as reopened, so
    /// cancelling the turn can give them back; the newly marked lines are
    /// returned for the caller to record under the right heading.
    fn apply(
        &self,
        snapshot: &Snapshot,
        request: &FileRequest<'_>,
        author: &MarkAuthor,
        marks: &mut TurnMarks,
    ) -> FileChange {
        let path = request.file.review_path().display();
        let (mut change, problem) = request.apply(&self.tracker, snapshot, author);
        marks.reopened.append(&mut change.reopened);
        if let Some(error) = problem {
            marks.problem.get_or_insert(format!("{path}: {error}"));
        }
        let _ = self.events.send(ui_events::ReviewStateSaved {
            review_unit: snapshot.identity.review_unit().clone(),
            path,
            result: self.tracker.status(snapshot, request.file).map_err(|_| ()),
        });
        change
    }
}

impl<'a> FileRequest<'a> {
    /// The marks of a turn grouped by the changed file they name, in
    /// snapshot order.
    fn group(
        snapshot: &'a Snapshot,
        reviewed: &'a [CodeLocation],
        reopened: &'a [CodeLocation],
    ) -> Vec<Self> {
        snapshot
            .files
            .iter()
            .filter_map(|file| {
                let names = |location: &&CodeLocation| location.names(file);
                let request = Self {
                    file,
                    reviewed: reviewed.iter().filter(names).collect(),
                    reopened: reopened.iter().filter(names).collect(),
                };
                (!request.reviewed.is_empty() || !request.reopened.is_empty()).then_some(request)
            })
            .collect()
    }

    /// Reopen, then mark, and report the lines that changed state, also
    /// when a step failed after another succeeded.
    fn apply(
        &self,
        tracker: &ReviewTracker,
        snapshot: &Snapshot,
        author: &MarkAuthor,
    ) -> (FileChange, Option<eyre::Report>) {
        let before = match tracker.lines(snapshot, self.file) {
            Ok(before) => before,
            Err(error) => return (FileChange::default(), Some(error)),
        };
        if before == FileLines::default() {
            return match self.apply_whole(tracker, snapshot, author) {
                Ok(change) => (change, None),
                Err(error) => (FileChange::default(), Some(error)),
            };
        }
        let problem = self.edit(tracker, snapshot, author, &before).err();
        match tracker.lines(snapshot, self.file) {
            Ok(after) => (
                FileChange::between(self.file, &before, &after, author),
                problem,
            ),
            Err(error) => (FileChange::default(), Some(problem.unwrap_or(error))),
        }
    }

    /// Reopen, then mark. A whole-file mark also covers open changes with
    /// no line to name: reviewed lines the current file deleted.
    fn edit(
        &self,
        tracker: &ReviewTracker,
        snapshot: &Snapshot,
        author: &MarkAuthor,
        before: &FileLines,
    ) -> eyre::Result<()> {
        let reopen =
            NamedLines::of(self.reopened.iter().copied()).or_whole(|| before.reviewed.selection());
        if !reopen.is_empty() {
            tracker.reopen_lines(snapshot, self.file, &reopen)?;
        }
        let accept =
            NamedLines::of(self.reviewed.iter().copied()).or_whole(|| before.open_selection());
        if !accept.is_empty() {
            tracker.accept_lines(snapshot, self.file, &accept, author)?;
        }
        if self
            .reviewed
            .iter()
            .any(|location| location.lines.is_none())
            && !tracker.lines(snapshot, self.file)?.open.is_empty()
        {
            // Hunk by hunk, so the lines others marked keep their authors.
            tracker.review_hunks_where(snapshot, self.file, true, author, |_| true)?;
        }
        Ok(())
    }

    /// Marks on a file without lines to mark one by one: binary and other
    /// non-text changes are marked or reopened whole.
    fn apply_whole(
        &self,
        tracker: &ReviewTracker,
        snapshot: &Snapshot,
        author: &MarkAuthor,
    ) -> eyre::Result<FileChange> {
        let mut change = FileChange::default();
        let whole = CodeLocation {
            path: self.file.review_path().clone(),
            side: SourceSide::New,
            lines: None,
        };
        let marked_by = tracker.whole_file_author(snapshot, self.file)?;
        if let Some(previous) = marked_by.clone().filter(|_| !self.reopened.is_empty()) {
            tracker.unreview(snapshot, self.file)?;
            change.reopened.push(ReopenedLines {
                location: whole.clone(),
                author: previous,
            });
        }
        let reviewed = marked_by.is_some() && change.reopened.is_empty();
        if !self.reviewed.is_empty() && !reviewed {
            tracker.mark(snapshot, self.file, author)?;
            change.reviewed.push(whole);
        }
        Ok(change)
    }
}

/// Consecutive lines with the same value, in order.
pub(crate) fn runs<T: Clone + Eq>(lines: impl Iterator<Item = (u32, T)>) -> Vec<(Range<u32>, T)> {
    let mut runs: Vec<(Range<u32>, T)> = Vec::new();
    for (line, value) in lines {
        match runs.last_mut() {
            Some((range, last)) if range.end == line && *last == value => range.end += 1,
            _ => runs.push((line..line + 1, value)),
        }
    }
    runs
}

// A child module, so giving marks back shares how marks name lines.
#[path = "unmark.rs"]
mod unmark;

#[cfg(test)]
#[path = "marks.tests.rs"]
mod tests;
