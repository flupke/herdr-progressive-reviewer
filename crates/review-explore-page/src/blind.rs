//! The blind first pick: on a question that is hard to reverse, the page hides the agent's
//! recommendation, and mixes the order of the choices, until the reviewer has picked one. The
//! pick posts to the page, which keeps it in a cookie until the answer carries it. The comment
//! the reviewer typed with the pick stays on the server, under an ID that the cookie carries,
//! so that the answer's form shows it again however long it is.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

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
    /// The ID under which [`PickComments`] keeps the comment typed with the pick; empty in a
    /// cookie set before the page kept comments.
    #[serde(default)]
    comment_id: String,
}

impl FirstPick {
    const COOKIE: &str = "explore_first_pick";

    /// The reviewer's pick of `choice` on the blind question `blind` in the round `round`, when
    /// it is the first, with the `comment` typed beside it, which `comments` keeps: `None` when
    /// the question does not offer `choice`, or has a first pick in the request's cookie
    /// already, which stays the first with its own comment.
    pub(crate) fn to_keep(
        headers: &HeaderMap,
        round: &str,
        blind: &BlindQuestion<'_>,
        choice: String,
        comments: &PickComments,
        comment: String,
    ) -> Option<Self> {
        let picked = Self::read(headers, Some(round), blind).is_some();
        (blind.offers(&choice) && !picked).then(|| Self {
            round: round.to_owned(),
            question: blind.0.id.clone(),
            version: blind.0.version,
            choice,
            comment_id: comments.keep(comment),
        })
    }

    /// The pick a request's cookie carries, when it was made on this version of the blind
    /// question `blind` in the round `round`.
    pub(crate) fn read(
        headers: &HeaderMap,
        round: Option<&str>,
        blind: &BlindQuestion<'_>,
    ) -> Option<Self> {
        let encoded = cookie(headers, Self::COOKIE)?;
        let pick: Self = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).ok()?).ok()?;
        (Some(pick.round.as_str()) == round && blind.0.is_version(&pick.question, pick.version))
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

/// The comments the reviewer typed with the first pick of a blind question, kept from the pick
/// to the answer. A comment is never part of an answer until the reviewer sends it.
#[derive(Default)]
pub(crate) struct PickComments(Mutex<VecDeque<PickComment>>);

/// One comment typed with a first pick, under the ID its pick's cookie carries.
struct PickComment {
    id: String,
    text: String,
}

impl PickComments {
    /// The most comments kept. A pick whose answer is never sent from the page, because the
    /// reviewer answered in the pane or the round moved on, leaves its comment behind until
    /// later picks push it out.
    const KEPT: usize = 64;

    /// Keeps `comment` under a new ID, which it returns.
    fn keep(&self, comment: String) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut comments = self.lock();
        comments.push_back(PickComment {
            id: id.clone(),
            text: comment,
        });
        while comments.len() > Self::KEPT {
            comments.pop_front();
        }
        id
    }

    /// The comment typed with `pick`; empty when none is kept.
    pub(crate) fn of(&self, pick: &FirstPick) -> String {
        self.lock()
            .iter()
            .find(|kept| kept.id == pick.comment_id)
            .map(|kept| kept.text.clone())
            .unwrap_or_default()
    }

    /// Settles the comment of `pick` once its answer was posted: dropped when the answer was
    /// `sent`, or else replaced with the answer's `comment`, which the page shows again.
    pub(crate) fn settle(&self, pick: &FirstPick, sent: bool, comment: String) {
        let mut comments = self.lock();
        if sent {
            comments.retain(|kept| kept.id != pick.comment_id);
        } else if let Some(kept) = comments.iter_mut().find(|kept| kept.id == pick.comment_id) {
            kept.text = comment;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<PickComment>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
