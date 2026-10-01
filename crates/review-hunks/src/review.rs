//! Edits of the reviewed version: accepting and reopening hunks, and moving it
//! onto a new base.

use crate::hunks::HunkSpan;
use crate::text::{Lines, changes, splice, translate};

/// The three versions of one file that decide which of its hunks are
/// reviewed: the base, the reviewed version and the current file.
pub struct HunkReview<'a> {
    pub(crate) base: &'a [u8],
    pub(crate) reviewed: &'a [u8],
    pub(crate) current: &'a [u8],
}

/// The reviewed version after one hunk was accepted or reopened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewedVersion {
    /// Every line is reviewed: the reviewed version is the current file.
    Current,
    /// No line is reviewed: the reviewed version is the base.
    Base,
    /// Some hunks are reviewed.
    Partial(Vec<u8>),
}

impl<'a> HunkReview<'a> {
    /// Relate the versions of one file. `reviewed` must already sit on `base`
    /// (see [`replay`]).
    pub fn new(base: &'a [u8], reviewed: &'a [u8], current: &'a [u8]) -> Self {
        Self {
            base,
            reviewed,
            current,
        }
    }

    /// Accept one hunk of the open diff (reviewed version to current file):
    /// the reviewed version takes the hunk's current lines.
    pub fn review(&self, open: &HunkSpan) -> Option<ReviewedVersion> {
        let text = splice(
            &Lines::new(self.reviewed),
            open.old.clone(),
            &Lines::new(self.current),
            open.new.clone(),
        )?;
        Some(self.version(text))
    }

    /// Reopen one reviewed hunk: the reviewed version takes the hunk's base
    /// lines back. The hunk's current
    /// lines match the reviewed version, so they move to it untouched.
    pub fn unreview(&self, reviewed: &HunkSpan) -> Option<ReviewedVersion> {
        let open = changes(self.reviewed, self.current);
        let range = translate(
            open.iter().map(|change| (&change.after, &change.before)),
            &reviewed.new,
        )?;
        let text = splice(
            &Lines::new(self.reviewed),
            range,
            &Lines::new(self.base),
            reviewed.old.clone(),
        )?;
        Some(self.version(text))
    }

    fn version(&self, text: Vec<u8>) -> ReviewedVersion {
        if text == self.current {
            ReviewedVersion::Current
        } else if text == self.base {
            ReviewedVersion::Base
        } else {
            ReviewedVersion::Partial(text)
        }
    }
}

/// Move a reviewed version from the base it was built on to a new base. The
/// reviewed changes land on the new base; a reviewed change that meets an
/// upstream change is dropped, so its lines read as unreviewed again rather
/// than as silently reviewed.
pub fn replay(old_base: &[u8], reviewed: &[u8], new_base: &[u8]) -> Vec<u8> {
    if old_base == new_base {
        return reviewed.to_vec();
    }
    let upstream = changes(old_base, new_base);
    let reviewed_lines = Lines::new(reviewed);
    let base_lines = Lines::new(new_base);
    let mut text = Vec::new();
    let mut position = 0;
    for change in changes(old_base, reviewed) {
        let Some(target) = translate(
            upstream.iter().map(|change| (&change.before, &change.after)),
            &change.before,
        )
        .filter(|target| target.start >= position) else {
            continue;
        };
        let kept = base_lines.get(position..target.start).unwrap_or_default();
        let replaced = reviewed_lines.get(change.after).unwrap_or_default();
        for line in kept.iter().chain(replaced) {
            text.extend_from_slice(line);
        }
        position = target.end;
    }
    for line in base_lines.rest(position).unwrap_or_default() {
        text.extend_from_slice(line);
    }
    text
}

#[cfg(test)]
#[path = "review.tests.rs"]
mod tests;
