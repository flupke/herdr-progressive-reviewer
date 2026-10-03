//! The numbers of a set of rounds.
use crate::{proposals::ProposalCounts, sample::RoundSample};
use std::{cmp::Ordering, time::Duration};

/// How many of `whole` items were `part`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Share {
    pub(crate) part: usize,
    pub(crate) whole: usize,
}

impl Share {
    pub(crate) fn add(&mut self, counts: bool) {
        self.whole += 1;
        self.part += usize::from(counts);
    }

    /// The share as a ratio, when there is a whole.
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn ratio(self) -> Option<f64> {
        (self.whole > 0).then(|| self.part as f64 / self.whole as f64)
    }

    fn sum(shares: impl IntoIterator<Item = Self>) -> Self {
        shares
            .into_iter()
            .fold(Self::default(), |total, share| Self {
                part: total.part + share.part,
                whole: total.whole + share.whole,
            })
    }
}

/// The median and range of a count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spread {
    pub(crate) median: f64,
    pub(crate) least: usize,
    pub(crate) most: usize,
}

impl Spread {
    fn of(counts: impl IntoIterator<Item = usize>) -> Option<Self> {
        let sorted = Sorted::of(counts)?;
        Some(Self {
            median: sorted.median(),
            least: sorted.least(),
            most: sorted.most(),
        })
    }
}

/// Some values, at least one, in order.
struct Sorted<T>(Vec<T>);

impl<T: Copy> Sorted<T> {
    fn by(
        values: impl IntoIterator<Item = T>,
        order: impl FnMut(&T, &T) -> Ordering,
    ) -> Option<Self> {
        let mut values: Vec<T> = values.into_iter().collect();
        values.sort_unstable_by(order);
        (!values.is_empty()).then_some(Self(values))
    }

    fn least(&self) -> T {
        self.0[0]
    }

    fn most(&self) -> T {
        self.0[self.0.len() - 1]
    }

    /// The middle value twice, or the two middle values.
    fn middle(&self) -> (T, T) {
        let upper = self.0[self.0.len() / 2];
        if self.0.len().is_multiple_of(2) {
            (self.0[self.0.len() / 2 - 1], upper)
        } else {
            (upper, upper)
        }
    }
}

impl<T: Copy + Ord> Sorted<T> {
    fn of(values: impl IntoIterator<Item = T>) -> Option<Self> {
        Self::by(values, Ord::cmp)
    }
}

impl Sorted<usize> {
    #[allow(clippy::cast_precision_loss)]
    fn median(&self) -> f64 {
        let (lower, upper) = self.middle();
        (lower + upper) as f64 / 2.0
    }

    fn median_count(counts: impl IntoIterator<Item = usize>) -> Option<f64> {
        Some(Self::of(counts)?.median())
    }
}

impl Sorted<u64> {
    /// The median of some times in milliseconds.
    fn median_time(milliseconds: impl IntoIterator<Item = u64>) -> Option<Duration> {
        let (lower, upper) = Self::of(milliseconds)?.middle();
        Some(Duration::from_millis(lower.midpoint(upper)))
    }
}

impl Sorted<f64> {
    fn median_ratio(ratios: impl IntoIterator<Item = f64>) -> Option<f64> {
        let (lower, upper) = Self::by(ratios, f64::total_cmp)?.middle();
        Some(f64::midpoint(lower, upper))
    }
}

/// The numbers of some rounds. A round with no answer counts as a round and
/// stays out of every number about answers.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Summary {
    pub(crate) rounds: usize,
    pub(crate) answered: usize,
    pub(crate) concluded: usize,
    /// Over answered rounds: an unanswered round has at most its first question.
    pub(crate) questions: Option<Spread>,
    pub(crate) change_requests: Share,
    pub(crate) declined_recommendations: Share,
    pub(crate) agent_turn: Option<Duration>,
    pub(crate) agent_turn_after_answer: Option<Duration>,
    pub(crate) reviewer_answer: Option<Duration>,
    /// The median, over answered rounds, of the share of the round's time,
    /// agent turns plus reviewer answers, that went to agent turns.
    pub(crate) waiting_share: Option<f64>,
    pub(crate) question_words: Option<f64>,
    /// Over the rounds with a Challenger; none without one.
    pub(crate) proposals: Option<ProposalCounts>,
}

impl Summary {
    pub(crate) fn of<'a>(samples: impl IntoIterator<Item = &'a RoundSample>) -> Self {
        let samples: Vec<&RoundSample> = samples.into_iter().collect();
        let answered: Vec<&RoundSample> = samples
            .iter()
            .copied()
            .filter(|sample| sample.answered)
            .collect();
        let turns = || samples.iter().flat_map(|sample| &sample.agent_turns);
        Self {
            rounds: samples.len(),
            answered: answered.len(),
            concluded: samples.iter().filter(|sample| sample.concluded).count(),
            questions: Spread::of(answered.iter().map(|sample| sample.questions)),
            change_requests: Share::sum(answered.iter().map(|sample| sample.change_requests)),
            declined_recommendations: Share::sum(
                answered
                    .iter()
                    .map(|sample| sample.declined_recommendations),
            ),
            agent_turn: Sorted::median_time(turns().map(|turn| turn.milliseconds)),
            agent_turn_after_answer: Sorted::median_time(
                turns()
                    .filter(|turn| turn.after_answer)
                    .map(|turn| turn.milliseconds),
            ),
            reviewer_answer: Sorted::median_time(
                answered
                    .iter()
                    .flat_map(|sample| sample.reviewer_answers_ms.iter().copied()),
            ),
            waiting_share: Self::waiting_share(&answered),
            question_words: Sorted::median_count(
                samples
                    .iter()
                    .flat_map(|sample| sample.question_words.iter().copied()),
            ),
            proposals: ProposalCounts::sum(
                samples
                    .iter()
                    .filter(|sample| sample.challenger)
                    .map(|sample| sample.proposals),
            ),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn waiting_share(answered: &[&RoundSample]) -> Option<f64> {
        Sorted::median_ratio(
            answered
                .iter()
                .filter(|sample| !sample.reviewer_answers_ms.is_empty())
                .map(|sample| {
                    let agent: u64 = sample.agent_turns.iter().map(|t| t.milliseconds).sum();
                    let reviewer: u64 = sample.reviewer_answers_ms.iter().sum();
                    agent as f64 / (agent + reviewer).max(1) as f64
                }),
        )
    }
}
