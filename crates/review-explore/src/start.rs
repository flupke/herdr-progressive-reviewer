//! Whether the reviewer can start an Explore round on a review checkpoint: the one rule that the
//! pane, the Explore page and the session follow.

use std::fmt;

/// Why the reviewer cannot start an Explore round on a review checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartBlock {
    /// Every changed line is marked as reviewed: a round would have nothing to ask about.
    NothingToReview,
}

impl StartBlock {
    /// What stops a round on a checkpoint whose changed files say, each, whether they still hold
    /// unreviewed lines; `None` when a round can start. A round needs unreviewed lines to ask
    /// about, so a checkpoint whose changed files are all reviewed cannot start one. A
    /// checkpoint with no changed file has no reviewed lines either, and is left to start.
    pub fn of(unreviewed: impl IntoIterator<Item = bool>) -> Option<Self> {
        let mut changed = false;
        for unreviewed in unreviewed {
            if unreviewed {
                return None;
            }
            changed = true;
        }
        changed.then_some(Self::NothingToReview)
    }

    /// Why no round can start, in a sentence the pane and the page show beside the inactive
    /// start buttons.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NothingToReview => {
                "Nothing is left to review: every changed line is marked as reviewed. Unmark \
                 lines or files to start a round."
            }
        }
    }
}

impl fmt::Display for StartBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.reason())
    }
}

#[cfg(test)]
mod tests {
    use super::StartBlock;

    #[test]
    fn a_round_needs_a_changed_file_with_unreviewed_lines() {
        assert_eq!(
            StartBlock::of([false, false]),
            Some(StartBlock::NothingToReview)
        );
        assert_eq!(StartBlock::of([false, true]), None);
        assert_eq!(StartBlock::of([true]), None);
        assert_eq!(StartBlock::of([]), None, "a change with no file");
    }
}
