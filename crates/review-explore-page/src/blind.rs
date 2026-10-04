//! The blind first pick: on a question that is hard to reverse, the page hides the agent's
//! recommendation, and mixes the order of the choices, until the reviewer has picked one. The
//! page keeps the pick, once the round's owner says that the round still asks the question,
//! and the answer carries it. The comment typed with the pick stays in the answer's comment box,
//! which keeps the same draft.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};

use review_explore::{Alternative, Door, Question};

/// A question that hides the agent's recommendation until the reviewer's first pick.
pub(crate) struct BlindQuestion<'a>(&'a Question);

impl<'a> BlindQuestion<'a> {
    /// `question`, when its first pick is blind: its Door is mixed, one-way or unknown, and a
    /// choice carries a recommendation to hide. A two-way Door, or no assessment, shows the
    /// recommendation at once.
    pub(crate) fn of(question: &'a Question) -> Option<Self> {
        let door = question.assessments.as_ref()?.door;
        let recommends = question
            .alternatives
            .iter()
            .any(|alternative| alternative.recommendation.is_some());
        (door != Door::TwoWay && recommends).then_some(Self(question))
    }

    /// The agent's alternatives in a mixed order, then None of the above. The order depends
    /// only on the question's and the alternatives' IDs, so it is the same each time the page
    /// shows the question, a clarified version included.
    pub(crate) fn choices(&self) -> Vec<&'a Alternative> {
        let mut choices: Vec<_> = self.0.choices().collect();
        let none_of_the_above = choices.pop();
        choices.sort_by_key(|choice| Self::rank(&self.0.id, &choice.id));
        choices.extend(none_of_the_above);
        choices
    }

    /// Whether `choice` is the ID of one of the question's choices.
    pub(crate) fn offers(&self, choice: &str) -> bool {
        self.0.choice(choice).is_some()
    }

    /// A stable hash of the two IDs (FNV-1a), which a later Rust release does not change.
    fn rank(question: &str, choice: &str) -> u64 {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0100_0000_01b3;
        question
            .bytes()
            .chain([0xff])
            .chain(choice.bytes())
            .fold(OFFSET, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(PRIME)
            })
    }
}

/// The choices the reviewer picked first on blind questions, as this page keeps them: one per
/// version of a question of a round, which the answer carries to the round's owner. Every tab
/// of the page sees the pick. Each pick kept wakes the page's sockets, and counts for its round,
/// so that a page shows a newer view of its round only.
#[derive(Default)]
pub(crate) struct FirstPicks {
    kept: Mutex<Kept>,
    changes: tokio::sync::watch::Sender<()>,
}

/// The picks kept, and how many each round had so far.
#[derive(Default)]
struct Kept {
    picks: VecDeque<KeptPick>,
    /// Only goes up: a pick pushed out still counts.
    counts: HashMap<String, u64>,
}

/// One first pick, of the version `version` of the question `question` of the round `round`.
struct KeptPick {
    round: String,
    question: String,
    version: u32,
    choice: String,
}

impl KeptPick {
    fn of(&self, round: &str, question: &str, version: u32) -> bool {
        self.round == round && self.question == question && self.version == version
    }
}

/// What became of a pick the reviewer sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Pick {
    /// It is the question's first pick now.
    First,
    /// The question had a first pick already, which stays: this pick changed nothing.
    Already,
}

impl FirstPicks {
    /// The most picks kept: a pick whose answer is never sent from the page stays until later
    /// picks push it out.
    const KEPT: usize = 64;

    /// The first pick of the version `version` of the question `question` of the round `round`.
    pub(crate) fn of(&self, round: &str, question: &str, version: u32) -> Option<String> {
        self.lock()
            .picks
            .iter()
            .find(|kept| kept.of(round, question, version))
            .map(|kept| kept.choice.clone())
    }

    /// How many picks the round `round` had so far.
    pub(crate) fn count(&self, round: Option<&str>) -> u64 {
        round
            .and_then(|round| self.lock().counts.get(round).copied())
            .unwrap_or(0)
    }

    /// Keeps `choice` as the first pick of the blind question `blind` of the round `round`,
    /// unless it has one already. `choice` must be one of the question's choices.
    pub(crate) fn keep(&self, round: &str, blind: &BlindQuestion<'_>, choice: &str) -> Pick {
        let question = blind.0;
        let mut kept = self.lock();
        if kept
            .picks
            .iter()
            .any(|kept| kept.of(round, &question.id, question.version))
        {
            return Pick::Already;
        }
        kept.picks.push_back(KeptPick {
            round: round.to_owned(),
            question: question.id.clone(),
            version: question.version,
            choice: choice.to_owned(),
        });
        while kept.picks.len() > Self::KEPT {
            kept.picks.pop_front();
        }
        *kept.counts.entry(round.to_owned()).or_default() += 1;
        drop(kept);
        self.changes.send_replace(());
        Pick::First
    }

    /// Wakes up with each pick kept.
    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<()> {
        self.changes.subscribe()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
