//! Who accepted the changes a reviewed version holds.

use std::collections::BTreeMap;
use std::ops::Range;

use review_types::MarkAuthor;

use crate::text::{Lines, changes};

/// Consecutive lines one author accepted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredLines {
    /// Zero-based lines.
    pub lines: Range<u32>,
    pub author: MarkAuthor,
}

/// Who accepted each change of a reviewed version. The base lines it removes
/// are numbered like the base and the lines it adds like the reviewed
/// version. A changed line without an entry was accepted by `default`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Attribution {
    pub default: MarkAuthor,
    /// Base lines the reviewed version removes, by a non-default author.
    pub removed: Vec<AuthoredLines>,
    /// Lines the reviewed version adds, by a non-default author.
    pub added: Vec<AuthoredLines>,
}

/// A reviewed version and who accepted its changes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Reviewed {
    pub text: Vec<u8>,
    pub attribution: Attribution,
}

/// The attribution of a reviewed version whose changes the reviewer
/// accepted, the same as [`Attribution::default`] but borrowable for any
/// lifetime.
pub(crate) static REVIEWER_ONLY: Attribution = Attribution {
    default: MarkAuthor::Reviewer,
    removed: Vec::new(),
    added: Vec::new(),
};

impl Attribution {
    /// Every change accepted by one author.
    pub fn uniform(author: MarkAuthor) -> Self {
        Self {
            default: author,
            removed: Vec::new(),
            added: Vec::new(),
        }
    }

    /// The one author of every change, when there is only one.
    pub fn uniform_author(&self) -> Option<&MarkAuthor> {
        (self.removed.is_empty() && self.added.is_empty()).then_some(&self.default)
    }

    /// Who accepted the removal of one base line.
    pub fn removed_by(&self, line: u32) -> &MarkAuthor {
        Self::author(&self.removed, line).unwrap_or(&self.default)
    }

    /// Who accepted one added line of the reviewed version.
    pub fn added_by(&self, line: u32) -> &MarkAuthor {
        Self::author(&self.added, line).unwrap_or(&self.default)
    }

    fn author(entries: &[AuthoredLines], line: u32) -> Option<&MarkAuthor> {
        entries
            .iter()
            .find(|entry| entry.lines.contains(&line))
            .map(|entry| &entry.author)
    }

    /// Runs of `authors` that differ from the default, starting at line 0.
    fn runs(&self, authors: impl IntoIterator<Item = (u32, MarkAuthor)>) -> Vec<AuthoredLines> {
        let mut runs: Vec<AuthoredLines> = Vec::new();
        for (line, author) in authors {
            if author == self.default {
                continue;
            }
            match runs.last_mut() {
                Some(last) if last.lines.end == line && last.author == author => {
                    last.lines.end += 1;
                }
                _ => runs.push(AuthoredLines {
                    lines: line..line + 1,
                    author,
                }),
            }
        }
        runs
    }
}

/// A new reviewed version written line by line, remembering who accepted
/// each line in case it turns out to be a change from the base.
pub(crate) struct Rebuild {
    default: MarkAuthor,
    text: Vec<u8>,
    /// The author of each written line, should it be an added line.
    authors: Vec<MarkAuthor>,
    /// Who accepted the removal of base lines, should they be removed.
    removed: BTreeMap<u32, MarkAuthor>,
}

impl Rebuild {
    /// Start from the authors of base removals `previous` already holds,
    /// numbered like the same base.
    pub(crate) fn on(previous: &Attribution) -> Self {
        let mut rebuild = Self::with_default(previous.default.clone());
        for entry in &previous.removed {
            for line in entry.lines.clone() {
                rebuild.remove(line, &entry.author);
            }
        }
        rebuild
    }

    /// Start without any known base removals.
    pub(crate) fn with_default(default: MarkAuthor) -> Self {
        Self {
            default,
            text: Vec::new(),
            authors: Vec::new(),
            removed: BTreeMap::new(),
        }
    }

    pub(crate) fn push(&mut self, line: &[u8], author: &MarkAuthor) {
        self.text.extend_from_slice(line);
        self.authors.push(author.clone());
    }

    /// Copy `range` of the reviewed version `lines`, with the authors
    /// `attribution` gives the ones it added; `None` when `range` is not
    /// within `lines`.
    pub(crate) fn keep(
        &mut self,
        lines: &Lines<'_>,
        range: Range<u32>,
        attribution: &Attribution,
    ) -> Option<()> {
        for (line, text) in range.clone().zip(lines.get(range)?) {
            self.push(text, attribution.added_by(line));
        }
        Some(())
    }

    /// Record who accepted the removal of one base line.
    pub(crate) fn remove(&mut self, line: u32, author: &MarkAuthor) {
        self.removed.insert(line, author.clone());
    }

    /// The reviewed version, attributing only the lines that differ from
    /// `base`. When one author accepted every change, they become the default.
    pub(crate) fn finish(self, base: &[u8]) -> Reviewed {
        let changes = changes(base, &self.text);
        let added = changes
            .iter()
            .flat_map(|change| {
                change.after.clone().map(|line| {
                    let author = usize::try_from(line)
                        .ok()
                        .and_then(|index| self.authors.get(index))
                        .unwrap_or(&self.default);
                    (line, author.clone())
                })
            })
            .collect::<Vec<_>>();
        let removed = changes
            .iter()
            .flat_map(|change| {
                change.before.clone().map(|line| {
                    let author = self.removed.get(&line).unwrap_or(&self.default);
                    (line, author.clone())
                })
            })
            .collect::<Vec<_>>();
        let mut authors = added.iter().chain(&removed).map(|(_, author)| author);
        let default = match authors.next() {
            Some(first) if authors.all(|author| author == first) => first.clone(),
            _ => self.default,
        };
        let mut attribution = Attribution::uniform(default);
        attribution.added = attribution.runs(added);
        attribution.removed = attribution.runs(removed);
        Reviewed {
            text: self.text,
            attribution,
        }
    }
}

#[cfg(test)]
#[path = "attribution.tests.rs"]
mod tests;
