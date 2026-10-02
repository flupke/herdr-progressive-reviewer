//! Edits of the reviewed version: accepting and reopening hunks, and moving it
//! onto a new base.

use review_types::MarkAuthor;

use crate::attribution::{Attribution, REVIEWER_ONLY, Rebuild, Reviewed};
use crate::hunks::HunkSpan;
use crate::text::{Lines, before_line, changes, overlaps, translate};

/// The three versions of one file that decide which of its hunks are
/// reviewed: the base, the reviewed version and the current file, with who
/// accepted the reviewed version's changes.
pub struct HunkReview<'a> {
    pub(crate) base: &'a [u8],
    pub(crate) reviewed: &'a [u8],
    pub(crate) current: &'a [u8],
    pub(crate) attribution: &'a Attribution,
}

/// The lines one open hunk changes, numbered like the base and the current file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangedLines {
    /// Zero-based base lines the hunk removes.
    pub removed: Vec<u32>,
    /// Zero-based current lines the hunk adds.
    pub added: Vec<u32>,
    /// The hunk removes reviewed lines the base does not have: it rewrites
    /// what the reviewer approved, which no change from the base shows.
    pub rewrites_reviewed: bool,
}

/// Where the lines of the reviewed version sit in the base.
pub struct BaseNumbering(Vec<crate::text::Change>);

impl BaseNumbering {
    /// The zero-based base line behind a zero-based line of the reviewed
    /// version, or `None` for a line a reviewed change added.
    pub fn base_line(&self, reviewed_line: u32) -> Option<u32> {
        before_line(&self.0, reviewed_line)
    }
}

/// The reviewed version after lines were accepted or reopened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewedVersion {
    /// Every line is reviewed: the reviewed version is the current file.
    Current(Attribution),
    /// No line is reviewed: the reviewed version is the base.
    Base,
    /// Some lines are reviewed.
    Partial(Reviewed),
}

impl<'a> HunkReview<'a> {
    /// Relate the versions of one file whose changes the reviewer accepted.
    /// `reviewed` must already sit on `base` (see [`replay`]).
    pub fn new(base: &'a [u8], reviewed: &'a [u8], current: &'a [u8]) -> Self {
        Self {
            base,
            reviewed,
            current,
            attribution: &REVIEWER_ONLY,
        }
    }

    /// Say who accepted the reviewed version's changes.
    #[must_use]
    pub fn attributed(self, attribution: &'a Attribution) -> Self {
        Self {
            attribution,
            ..self
        }
    }

    /// Accept one hunk of the open diff (reviewed version to current file):
    /// the reviewed version takes the hunk's current lines.
    pub fn review(&self, open: &HunkSpan, author: &MarkAuthor) -> Option<ReviewedVersion> {
        self.review_all(std::slice::from_ref(open), author)
    }

    /// Accept several hunks of the open diff at once, or none when two of
    /// them overlap. The unchanged lines a hunk spans keep their authors.
    pub fn review_all(&self, open: &[HunkSpan], author: &MarkAuthor) -> Option<ReviewedVersion> {
        let reviewed = Lines::new(self.reviewed);
        let current = Lines::new(self.current);
        let reviewed_changes = changes(self.base, self.reviewed);
        let mut spans = open.iter().collect::<Vec<_>>();
        spans.sort_by_key(|span| span.old.start);
        let mut rebuild = Rebuild::on(self.attribution);
        let mut position = 0;
        for span in spans {
            rebuild.keep(&reviewed, position..span.old.start, self.attribution)?;
            let replaced = reviewed.get(span.old.clone())?;
            let added = current.get(span.new.clone())?;
            let inside = changes(&replaced.concat(), &added.concat());
            for change in &inside {
                for line in change.before.clone() {
                    if let Some(base) = before_line(&reviewed_changes, span.old.start + line) {
                        rebuild.remove(base, author);
                    }
                }
            }
            for (offset, text) in (0..).zip(added) {
                let author = match before_line(&inside, offset) {
                    // An unchanged line inside the hunk keeps its author.
                    Some(kept) => self.attribution.added_by(span.old.start + kept),
                    None => author,
                };
                rebuild.push(text, author);
            }
            position = span.old.end;
        }
        rebuild.keep(&reviewed, position..reviewed.len(), self.attribution)?;
        Some(self.version(rebuild.finish(self.base)))
    }

    /// How to number the reviewed version's lines like the base.
    pub fn base_numbering(&self) -> BaseNumbering {
        BaseNumbering(changes(self.base, self.reviewed))
    }

    /// The lines each open hunk changes, zero-based: the base lines it
    /// removes and the current lines it adds.
    pub fn changed_lines(&self, open: &[HunkSpan]) -> Vec<ChangedLines> {
        let reviewed_changes = changes(self.base, self.reviewed);
        let open_changes = changes(self.reviewed, self.current);
        open.iter()
            .map(|span| {
                let mut lines = ChangedLines::default();
                for change in open_changes
                    .iter()
                    .filter(|change| overlaps(&change.before, &span.old))
                {
                    for line in change.before.clone() {
                        match before_line(&reviewed_changes, line) {
                            Some(base) => lines.removed.push(base),
                            None => lines.rewrites_reviewed = true,
                        }
                    }
                    lines.added.extend(change.after.clone());
                }
                lines
            })
            .collect()
    }

    /// Reopen one reviewed hunk: the reviewed version takes the hunk's base
    /// lines back. The hunk's current lines match the reviewed version, so
    /// they move to it untouched.
    pub fn unreview(&self, reviewed: &HunkSpan) -> Option<ReviewedVersion> {
        let open = changes(self.reviewed, self.current);
        let range = translate(
            open.iter().map(|change| (&change.after, &change.before)),
            &reviewed.new,
        )?;
        let reviewed_lines = Lines::new(self.reviewed);
        let base = Lines::new(self.base);
        let mut rebuild = Rebuild::on(self.attribution);
        rebuild.keep(&reviewed_lines, 0..range.start, self.attribution)?;
        for line in base.get(reviewed.old.clone())? {
            rebuild.push(line, &self.attribution.default);
        }
        rebuild.keep(
            &reviewed_lines,
            range.end..reviewed_lines.len(),
            self.attribution,
        )?;
        Some(self.version(rebuild.finish(self.base)))
    }

    pub(crate) fn version(&self, reviewed: Reviewed) -> ReviewedVersion {
        if reviewed.text == self.current {
            ReviewedVersion::Current(reviewed.attribution)
        } else if reviewed.text == self.base {
            ReviewedVersion::Base
        } else {
            ReviewedVersion::Partial(reviewed)
        }
    }
}

/// Move a reviewed version from the base it was built on to a new base. The
/// reviewed changes land on the new base with their authors; a reviewed
/// change that meets an upstream change is dropped, so its lines read as
/// unreviewed again rather than as silently reviewed.
pub fn replay(old_base: &[u8], reviewed: &Reviewed, new_base: &[u8]) -> Reviewed {
    if old_base == new_base {
        return reviewed.clone();
    }
    let attribution = &reviewed.attribution;
    let upstream = changes(old_base, new_base);
    let reviewed_lines = Lines::new(&reviewed.text);
    let base_lines = Lines::new(new_base);
    let mut rebuild = Rebuild::with_default(attribution.default.clone());
    let mut position = 0;
    for change in changes(old_base, &reviewed.text) {
        let Some(target) = translate(
            upstream
                .iter()
                .map(|change| (&change.before, &change.after)),
            &change.before,
        )
        .filter(|target| target.start >= position) else {
            continue;
        };
        for line in base_lines.get(position..target.start).unwrap_or_default() {
            rebuild.push(line, &attribution.default);
        }
        for (old, new) in change.before.clone().zip(target.clone()) {
            rebuild.remove(new, attribution.removed_by(old));
        }
        for (line, text) in change
            .after
            .clone()
            .zip(reviewed_lines.get(change.after.clone()).unwrap_or_default())
        {
            rebuild.push(text, attribution.added_by(line));
        }
        position = target.end;
    }
    for line in base_lines.rest(position).unwrap_or_default() {
        rebuild.push(line, &attribution.default);
    }
    rebuild.finish(new_base)
}

#[cfg(test)]
#[path = "review.tests.rs"]
mod tests;
