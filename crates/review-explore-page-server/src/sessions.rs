//! The sessions of the standalone server. Each stands in for the review tool's Explore session:
//! it owns one round, behind its own token, and its agent posts what its controller asks for.

use std::sync::{Arc, Mutex, PoisonError};

use review_explore_page::{RoundFeed, RoundPublisher, RoundStage, Rounds, Token};

use crate::fixed_question::question_stage;

#[derive(Clone, Default)]
pub(crate) struct Sessions(Arc<Mutex<Vec<Session>>>);

struct Session {
    token: Token,
    round: RoundPublisher,
}

impl Sessions {
    /// Opens a session whose round is at `stage`, behind `token`.
    pub(crate) fn open(&self, token: Token, stage: RoundStage) {
        self.lock().push(Session {
            token,
            round: RoundPublisher::new(stage),
        });
    }

    /// The agent of the session behind `token` posts the fixed question. Returns false when no
    /// session has that token.
    pub(crate) fn ask_question(&self, token: &str) -> bool {
        let sessions = self.lock();
        let Some(session) = find(&sessions, token) else {
            return false;
        };
        session.round.publish(question_stage());
        true
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Session>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Rounds for Sessions {
    fn find(&self, token: &str) -> Option<RoundFeed> {
        find(&self.lock(), token).map(|session| session.round.subscribe())
    }
}

fn find<'a>(sessions: &'a [Session], token: &str) -> Option<&'a Session> {
    sessions.iter().find(|session| session.token.matches(token))
}
