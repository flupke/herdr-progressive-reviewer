//! What became of the Challenger's proposals in some rounds.
use review_explore::{Exploration, ProposalResult};
use std::collections::HashMap;

/// How many of the Challenger's proposals ended each way. A proposal that
/// several turns report, as one kept and asked later, counts once, at the
/// result of the latest turn that reported it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ProposalCounts {
    /// The rounds that reported any proposal: a round saved before turns
    /// reported them has none, like a round whose Challenger proposed nothing.
    pub(crate) rounds: usize,
    pub(crate) asked: usize,
    pub(crate) merged: usize,
    pub(crate) retired: usize,
    pub(crate) kept: usize,
}

impl ProposalCounts {
    /// The proposals the turns of `exploration` reported, told apart by title.
    pub(crate) fn of(exploration: &Exploration) -> Self {
        let mut latest = HashMap::new();
        for proposal in exploration
            .conversation
            .iter()
            .flat_map(|turn| &turn.update.challenger_proposals)
        {
            latest.insert(proposal.title.trim(), proposal.result);
        }
        let mut counts = Self::default();
        for result in latest.into_values() {
            *counts.of_result(result) += 1;
        }
        counts.rounds = usize::from(counts.total() > 0);
        counts
    }

    fn of_result(&mut self, result: ProposalResult) -> &mut usize {
        match result {
            ProposalResult::Asked => &mut self.asked,
            ProposalResult::Merged => &mut self.merged,
            ProposalResult::Retired => &mut self.retired,
            ProposalResult::Kept => &mut self.kept,
        }
    }

    pub(crate) fn total(self) -> usize {
        self.asked + self.merged + self.retired + self.kept
    }

    /// The counts of several rounds, or none without a round.
    pub(crate) fn sum(counts: impl IntoIterator<Item = Self>) -> Option<Self> {
        counts.into_iter().reduce(|total, round| Self {
            rounds: total.rounds + round.rounds,
            asked: total.asked + round.asked,
            merged: total.merged + round.merged,
            retired: total.retired + round.retired,
            kept: total.kept + round.kept,
        })
    }
}
