//! The blind first pick: on a question that is hard to reverse, the page hides the agent's
//! recommendation, and mixes the order of the choices, until the reviewer has picked one. The
//! pick posts to the page, which keeps it in a cookie until the answer carries it.

use axum::http::{HeaderMap, HeaderValue};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use review_explore::{Alternative, Door, Question};
use serde::{Deserialize, Serialize};

use crate::access::cookie;

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

/// The choice the reviewer picked first on a blind question, by ID, kept in a cookie from the
/// pick to the answer.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct FirstPick {
    /// The identity of the round, so that a pick never carries over to another round's
    /// question of the same ID.
    round: String,
    question: String,
    version: u32,
    pub(crate) choice: String,
}

impl FirstPick {
    const COOKIE: &str = "explore_first_pick";

    /// The reviewer's pick of `choice` on `question` in the round `round`, when it is the
    /// first: `None` when the question is not blind, does not offer `choice`, or has a first
    /// pick in the request's cookie already, which stays the first.
    pub(crate) fn to_keep(
        headers: &HeaderMap,
        round: &str,
        question: &Question,
        choice: String,
    ) -> Option<Self> {
        let offered = BlindQuestion::of(question).is_some_and(|blind| blind.offers(&choice));
        let picked = Self::read(headers, Some(round), question).is_some();
        (offered && !picked).then(|| Self {
            round: round.to_owned(),
            question: question.id.clone(),
            version: question.version,
            choice,
        })
    }

    /// The pick a request's cookie carries, when it was made on this version of `question` in
    /// the round `round`.
    pub(crate) fn read(
        headers: &HeaderMap,
        round: Option<&str>,
        question: &Question,
    ) -> Option<Self> {
        let encoded = cookie(headers, Self::COOKIE)?;
        let pick: Self = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).ok()?).ok()?;
        (Some(pick.round.as_str()) == round && question.is_version(&pick.question, pick.version))
            .then_some(pick)
    }

    /// The `Set-Cookie` value that keeps the pick until the answer.
    pub(crate) fn cookie(&self) -> HeaderValue {
        let json = serde_json::to_vec(self).expect("a first pick serializes");
        // A day is longer than any answer takes; a pick of a question that no longer waits
        // is ignored anyway.
        Self::header(&URL_SAFE_NO_PAD.encode(json), 86_400)
    }

    /// The `Set-Cookie` value that drops the pick once its answer is sent.
    pub(crate) fn clear() -> HeaderValue {
        Self::header("", 0)
    }

    fn header(value: &str, max_age: u32) -> HeaderValue {
        // The value holds only letters, digits, `-` and `_`.
        HeaderValue::from_str(&format!(
            "{}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}",
            Self::COOKIE
        ))
        .expect("a first pick cookie is a valid header")
    }
}
