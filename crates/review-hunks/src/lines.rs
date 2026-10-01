//! Accepting and reopening individual changed lines. Hunks are only a view
//! of the reviewed version, so a hunk splits where a selection ends.

use std::collections::BTreeSet;
use std::ops::Range;

use review_types::MarkAuthor;

use crate::attribution::{Rebuild, Reviewed};
use crate::review::{HunkReview, ReviewedVersion};
use crate::text::{Change, Lines, after_line, before_line, changes, hunk_groups};

/// Changed lines picked from a file's diff from the base, zero-based: base
/// lines it removes and current lines it adds.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LineSelection {
    pub removed: BTreeSet<u32>,
    pub added: BTreeSet<u32>,
}

/// The picked lines of one change. `removed` and `added` follow the change's
/// before and after lines.
#[derive(Clone, Debug)]
struct Pick {
    removed: Vec<bool>,
    added: Vec<bool>,
}

impl Pick {
    fn any(&self) -> bool {
        self.removed.iter().chain(&self.added).any(|picked| *picked)
    }

    fn every(change: &Change) -> Self {
        Self {
            removed: vec![true; change.before.len()],
            added: vec![true; change.after.len()],
        }
    }

    fn none(change: &Change) -> Self {
        Self {
            removed: vec![false; change.before.len()],
            added: vec![false; change.after.len()],
        }
    }
}

impl HunkReview<'_> {
    /// Accept the open lines `selection` names; `None` when it names none.
    /// An accepted change keeps its unpicked lines open, so its hunk splits.
    /// Reviewed lines a change rewrites have no base number: they go only
    /// when every line of their change is picked.
    pub fn accept_lines(
        &self,
        selection: &LineSelection,
        author: &MarkAuthor,
    ) -> Option<ReviewedVersion> {
        let reviewed_changes = changes(self.base, self.reviewed);
        let open = changes(self.reviewed, self.current);
        let base_lines = |change: &Change| {
            change
                .before
                .clone()
                .map(|line| before_line(&reviewed_changes, line))
                .collect::<Vec<_>>()
        };
        let picks = open
            .iter()
            .map(|change| {
                let bases = base_lines(change);
                let mut pick = Pick {
                    removed: bases
                        .iter()
                        .map(|base| base.is_some_and(|base| selection.removed.contains(&base)))
                        .collect(),
                    added: change
                        .after
                        .clone()
                        .map(|line| selection.added.contains(&line))
                        .collect(),
                };
                // Rewritten reviewed lines leave with a fully picked change.
                let addressed = bases
                    .iter()
                    .zip(&pick.removed)
                    .filter(|(base, _)| base.is_some())
                    .map(|(_, picked)| *picked)
                    .chain(pick.added.iter().copied())
                    .collect::<Vec<_>>();
                if !addressed.is_empty() && addressed.iter().all(|picked| *picked) {
                    pick = Pick::every(change);
                }
                pick
            })
            .collect::<Vec<_>>();
        let accept = Accept {
            review: self,
            reviewed_changes: &reviewed_changes,
            open: &open,
            author,
        };
        let picks = settle(&open, picks, |picks| accept.matches(picks))?;
        Some(self.version(accept.build(&picks)?))
    }

    /// Reopen the reviewed lines `selection` names; `None` when it names
    /// none. Reviewed lines the current file rewrote are open already.
    pub fn reopen_lines(&self, selection: &LineSelection) -> Option<ReviewedVersion> {
        let reviewed_changes = changes(self.base, self.reviewed);
        let open = changes(self.reviewed, self.current);
        let picks = reviewed_changes
            .iter()
            .map(|change| Pick {
                removed: change
                    .before
                    .clone()
                    .map(|line| selection.removed.contains(&line))
                    .collect(),
                added: change
                    .after
                    .clone()
                    .map(|line| {
                        after_line(&open, line).is_some_and(|line| selection.added.contains(&line))
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let reopen = Reopen {
            review: self,
            reviewed_changes: &reviewed_changes,
            open: &open,
        };
        let picks = settle(&reviewed_changes, picks, |picks| reopen.matches(picks))?;
        Some(self.version(reopen.build(&picks)?))
    }
}

/// Settle which lines to change. Each hunk whose picked lines would not come
/// out exactly as picked (the new diff can pair up equal lines differently)
/// changes whole instead, so no hunk ends up half changed in a way nobody
/// picked. `None` when nothing is picked.
fn settle(
    changes: &[Change],
    mut picks: Vec<Pick>,
    matches: impl Fn(&[Pick]) -> bool,
) -> Option<Vec<Pick>> {
    if !picks.iter().any(Pick::any) {
        return None;
    }
    let touched = hunks(changes)
        .filter(|hunk| picks[hunk.clone()].iter().any(Pick::any))
        .collect::<Vec<_>>();
    for hunk in &touched {
        if !matches(&alone(changes, &picks, hunk)) {
            widen(changes, &mut picks, hunk);
        }
    }
    if !matches(&picks) {
        for hunk in &touched {
            widen(changes, &mut picks, hunk);
        }
    }
    Some(picks)
}

/// The picks of one hunk, with nothing picked elsewhere.
fn alone(changes: &[Change], picks: &[Pick], hunk: &Range<usize>) -> Vec<Pick> {
    changes
        .iter()
        .zip(picks)
        .enumerate()
        .map(|(index, (change, pick))| {
            if hunk.contains(&index) {
                pick.clone()
            } else {
                Pick::none(change)
            }
        })
        .collect()
}

/// Pick every line of one hunk.
fn widen(changes: &[Change], picks: &mut [Pick], hunk: &Range<usize>) {
    for index in hunk.clone() {
        picks[index] = Pick::every(&changes[index]);
    }
}

/// The positions of each hunk's changes in `changes`.
fn hunks(changes: &[Change]) -> impl Iterator<Item = Range<usize>> {
    let mut start = 0;
    hunk_groups(changes).map(move |group| {
        let hunk = start..start + group.len();
        start = hunk.end;
        hunk
    })
}

/// Accepting picked open lines into the reviewed version.
struct Accept<'a> {
    review: &'a HunkReview<'a>,
    reviewed_changes: &'a [Change],
    open: &'a [Change],
    author: &'a MarkAuthor,
}

impl Accept<'_> {
    /// The reviewed version with the picked lines accepted: each change keeps
    /// its unpicked reviewed lines, followed by its picked current lines.
    fn build(&self, picks: &[Pick]) -> Option<Reviewed> {
        let review = self.review;
        let reviewed = Lines::new(review.reviewed);
        let current = Lines::new(review.current);
        let mut rebuild = Rebuild::on(review.attribution);
        let mut position = 0;
        for (change, pick) in self.open.iter().zip(picks) {
            rebuild.keep(&reviewed, position..change.before.start, review.attribution)?;
            self.accept(&mut rebuild, (&reviewed, &current), change, pick)?;
            position = change.before.end;
        }
        rebuild.keep(&reviewed, position..reviewed.len(), review.attribution)?;
        Some(rebuild.finish(review.base))
    }

    /// Write one open change: its unpicked reviewed lines, then its picked
    /// current lines.
    fn accept(
        &self,
        rebuild: &mut Rebuild,
        (reviewed, current): (&Lines<'_>, &Lines<'_>),
        change: &Change,
        pick: &Pick,
    ) -> Option<()> {
        for (line, picked) in change.before.clone().zip(&pick.removed) {
            if !picked {
                rebuild.keep(reviewed, line..line + 1, self.review.attribution)?;
            } else if let Some(base) = before_line(self.reviewed_changes, line) {
                rebuild.remove(base, self.author);
            }
        }
        for (line, picked) in change.after.clone().zip(&pick.added) {
            if *picked {
                rebuild.push(current.line(line)?, self.author);
            }
        }
        Some(())
    }

    /// Whether accepting `picks` leaves open exactly the lines it did not pick.
    fn matches(&self, picks: &[Pick]) -> bool {
        let mut expected = LineSelection::default();
        for (change, pick) in self.open.iter().zip(picks) {
            for (line, picked) in change.before.clone().zip(&pick.removed) {
                if let Some(base) = before_line(self.reviewed_changes, line)
                    && !picked
                {
                    expected.removed.insert(base);
                }
            }
            for (line, picked) in change.after.clone().zip(&pick.added) {
                if !picked {
                    expected.added.insert(line);
                }
            }
        }
        self.build(picks).is_some_and(|rebuilt| {
            open_lines(self.review.base, &rebuilt.text, self.review.current) == expected
        })
    }
}

/// The open lines of a reviewed version: the base lines its open diff
/// removes and the current lines it adds.
fn open_lines(base: &[u8], reviewed: &[u8], current: &[u8]) -> LineSelection {
    let reviewed_changes = changes(base, reviewed);
    let mut named = LineSelection::default();
    for change in changes(reviewed, current) {
        named.removed.extend(
            change
                .before
                .clone()
                .filter_map(|line| before_line(&reviewed_changes, line)),
        );
        named.added.extend(change.after.clone());
    }
    named
}

/// Reopening picked reviewed lines.
struct Reopen<'a> {
    review: &'a HunkReview<'a>,
    reviewed_changes: &'a [Change],
    open: &'a [Change],
}

impl Reopen<'_> {
    /// The reviewed version with the picked lines reopened: each reviewed
    /// change takes back its picked base lines, followed by its unpicked
    /// added lines.
    fn build(&self, picks: &[Pick]) -> Option<Reviewed> {
        let review = self.review;
        let reviewed = Lines::new(review.reviewed);
        let base = Lines::new(review.base);
        let mut rebuild = Rebuild::on(review.attribution);
        let mut position = 0;
        for (change, pick) in self.reviewed_changes.iter().zip(picks) {
            rebuild.keep(&reviewed, position..change.after.start, review.attribution)?;
            for (line, picked) in change.before.clone().zip(&pick.removed) {
                if *picked {
                    rebuild.push(base.line(line)?, &review.attribution.default);
                }
            }
            for (line, picked) in change.after.clone().zip(&pick.added) {
                if !picked {
                    rebuild.keep(&reviewed, line..line + 1, review.attribution)?;
                }
            }
            position = change.after.end;
        }
        rebuild.keep(&reviewed, position..reviewed.len(), review.attribution)?;
        Some(rebuild.finish(review.base))
    }

    /// Whether reopening `picks` keeps reviewed exactly the lines it did not pick.
    fn matches(&self, picks: &[Pick]) -> bool {
        let mut expected = LineSelection::default();
        for (change, pick) in self.reviewed_changes.iter().zip(picks) {
            for (line, picked) in change.before.clone().zip(&pick.removed) {
                if !picked {
                    expected.removed.insert(line);
                }
            }
            for (line, picked) in change.after.clone().zip(&pick.added) {
                if !picked && let Some(current) = after_line(self.open, line) {
                    expected.added.insert(current);
                }
            }
        }
        self.build(picks).is_some_and(|rebuilt| {
            reviewed_lines(self.review.base, &rebuilt.text, self.review.current) == expected
        })
    }
}

/// The reviewed lines of a reviewed version that the current file still
/// shows: the base lines it removes and, numbered like the current file,
/// the lines it adds.
fn reviewed_lines(base: &[u8], reviewed: &[u8], current: &[u8]) -> LineSelection {
    let open = changes(reviewed, current);
    let mut named = LineSelection::default();
    for change in changes(base, reviewed) {
        named.removed.extend(change.before.clone());
        named.added.extend(
            change
                .after
                .clone()
                .filter_map(|line| after_line(&open, line)),
        );
    }
    named
}

#[cfg(test)]
#[path = "lines.tests.rs"]
mod tests;
