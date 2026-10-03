//! The sessions of the standalone server. Each stands in for the review tool's Explore session:
//! it owns one round, behind its own token, and moves it to the step its controller asks for.

use std::sync::{Arc, Mutex, PoisonError};

use review_explore_page::{RoundFeed, RoundPublisher, RoundStage, Rounds, Token};

use crate::fixed_question::{conclusion_stage, question_stage};

#[derive(Clone, Default)]
pub(crate) struct Sessions(Arc<Mutex<Vec<Session>>>);

struct Session {
    token: Token,
    round: RoundPublisher,
    /// The rounds the session started so far.
    rounds: usize,
    /// Whether the latest round is running: the reviewer has not reset it.
    running: bool,
    /// The questions the agent asked so far.
    asked: usize,
}

impl Session {
    /// The identity of the session's latest round.
    fn round_id(&self) -> String {
        self.rounds.to_string()
    }
}

/// What happens next in a session's round.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    /// The agent posts its next question.
    Question,
    /// The reviewer answered in the pane: the agent works on its next turn.
    Answer,
    /// The agent stops before its next turn.
    Interrupt,
    /// The agent concludes the round.
    Conclude,
    /// The reviewer resets the round: no round is running.
    Reset,
}

impl Step {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "question" => Self::Question,
            "answer" => Self::Answer,
            "interrupt" => Self::Interrupt,
            "conclude" => Self::Conclude,
            "reset" => Self::Reset,
            _ => return None,
        })
    }
}

impl Sessions {
    /// Opens a session behind `token` whose agent asked `asked` questions: it shows the latest,
    /// or the agent works on its first one.
    pub(crate) fn open(&self, token: Token, asked: usize) {
        let stage = match asked {
            0 => RoundStage::AgentWorking,
            asked => question_stage(asked),
        };
        let session = Session {
            token,
            round: RoundPublisher::default(),
            rounds: 1,
            running: true,
            asked,
        };
        session.round.publish(Some(&session.round_id()), stage);
        self.lock().push(session);
    }

    /// Moves the round of the session behind `token` one step. Returns false when no session
    /// has that token.
    pub(crate) fn step(&self, token: &str, step: Step) -> bool {
        let mut sessions = self.lock();
        let Some(session) = find(&mut sessions, token) else {
            return false;
        };
        if !session.running {
            // A step after a reset starts the session's next round.
            session.rounds += 1;
        }
        let stage = match step {
            Step::Question => {
                session.asked += 1;
                question_stage(session.asked)
            }
            Step::Answer => RoundStage::AgentWorking,
            Step::Interrupt => RoundStage::Interrupted,
            Step::Conclude => conclusion_stage(),
            Step::Reset => {
                session.asked = 0;
                RoundStage::NoRound
            }
        };
        session.running = stage != RoundStage::NoRound;
        let round = session.running.then(|| session.round_id());
        session.round.publish(round.as_deref(), stage);
        true
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Session>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Rounds for Sessions {
    fn find(&self, token: &str) -> Option<RoundFeed> {
        find(&mut self.lock(), token).map(|session| session.round.subscribe())
    }
}

fn find<'a>(sessions: &'a mut [Session], token: &str) -> Option<&'a mut Session> {
    sessions
        .iter_mut()
        .find(|session| session.token.matches(token))
}
