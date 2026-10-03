//! The sessions of the standalone server. Each stands in for the review tool's Explore session:
//! it owns one round, behind its own token, moves it to the step its controller asks for, and
//! takes the answers and the starts the reviewer sends from the page.

use std::sync::{Arc, Mutex, PoisonError};

use review_explore::{DiagramError, Question};
use review_explore_page::{
    CommandRefusal, CommandSender, PageCommand, PageRound, PublishedRound, RoundPublisher,
    RoundStage, Rounds, Token,
};
use serde::Serialize;

use crate::fixed_design;
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
    /// The stage of the agent's latest question, which a cancelled answer brings back.
    latest_question: Option<RoundStage>,
    /// The answers the reviewer sent from the page, in order.
    answers: Vec<SentAnswer>,
    /// The diagram errors the page reported, each once, as the review tool saves them with
    /// their question.
    diagram_errors: Vec<DiagramError>,
    /// The rounds the reviewer started from the page, in order.
    starts: Vec<SentStart>,
}

/// An answer the reviewer sent from the page, as a test reads it back.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SentAnswer {
    question: String,
    version: u32,
    choice: Option<String>,
    comment: String,
    /// The choice picked first on a blind question; left out when there was none.
    #[serde(skip_serializing_if = "Option::is_none")]
    first_pick: Option<String>,
}

/// A round the reviewer started from the page, as a test reads it back.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SentStart {
    challenger: bool,
}

impl Session {
    /// The identity of the session's latest round.
    fn round_id(&self) -> String {
        self.rounds.to_string()
    }

    /// Publishes `stage` of the latest round, with the fixed design once the agent asked its
    /// first question.
    fn publish(&self, stage: RoundStage) {
        let id = self.round_id();
        let design = fixed_design::design();
        let round = self.running.then(|| PublishedRound {
            id: &id,
            design: (self.asked > 0).then_some(&design),
        });
        self.round.publish(round, stage);
    }

    /// The stage that `step` moves the round to; `question`, when given, is the question the
    /// agent posts instead of the next fixed one. `None` when the agent asked no question whose
    /// answer could be cancelled.
    fn stage_after(&mut self, step: Step, question: Option<Question>) -> Option<RoundStage> {
        Some(match step {
            Step::Question => {
                self.asked += 1;
                let stage = question_stage(self.asked, question);
                self.latest_question = Some(stage.clone());
                stage
            }
            Step::Cancel => self.latest_question.clone()?,
            Step::Answer | Step::Kickoff => RoundStage::AgentWorking,
            Step::Fail => RoundStage::Interrupted {
                failure: Some("The selected agent is no longer available".into()),
            },
            Step::Interrupt => RoundStage::Interrupted { failure: None },
            Step::Conclude => conclusion_stage(),
            Step::Reset => RoundStage::NoRound,
            Step::FailStart => RoundStage::StartFailed {
                failure: "Repository comparison is not ready; retry Start".into(),
            },
        })
    }
}

/// What happens next in a session's round.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    /// The agent posts its next question.
    Question,
    /// The reviewer answered in the pane: the agent works on its next turn.
    Answer,
    /// The prompt of the agent's next turn could not be delivered.
    Fail,
    /// The reviewer cancels the latest answer in the pane: its question waits again.
    Cancel,
    /// The agent stops before its next turn.
    Interrupt,
    /// The agent concludes the round.
    Conclude,
    /// The reviewer resets the round: no round is running.
    Reset,
    /// The tool sent the kickoff of the round the reviewer started: the agent works on its first
    /// turn.
    Kickoff,
    /// The round the reviewer started could not start: no round is running.
    FailStart,
}

impl Step {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "question" => Self::Question,
            "answer" => Self::Answer,
            "fail" => Self::Fail,
            "cancel" => Self::Cancel,
            "interrupt" => Self::Interrupt,
            "conclude" => Self::Conclude,
            "reset" => Self::Reset,
            "kickoff" => Self::Kickoff,
            "fail-start" => Self::FailStart,
            _ => return None,
        })
    }
}

impl Sessions {
    /// Opens a session behind `token` whose agent asked `asked` questions: it shows the latest,
    /// or the agent works on its first one.
    pub(crate) fn open(&self, token: Token, asked: usize) {
        let latest_question = (asked > 0).then(|| question_stage(asked, None));
        let stage = latest_question.clone().unwrap_or(RoundStage::AgentWorking);
        let session = Session {
            token,
            round: RoundPublisher::default(),
            rounds: 1,
            running: true,
            asked,
            latest_question,
            answers: Vec::new(),
            diagram_errors: Vec::new(),
            starts: Vec::new(),
        };
        session.publish(stage);
        self.lock().push(session);
    }

    /// Moves the round of the session behind `token` one step; `question`, when given, is the
    /// question the agent posts instead of the next fixed one. Returns false when no session
    /// has that token, or when the agent asked no question whose answer could be cancelled.
    pub(crate) fn step(&self, token: &str, step: Step, question: Option<Question>) -> bool {
        let mut sessions = self.lock();
        let Some(session) = find(&mut sessions, token) else {
            return false;
        };
        let Some(stage) = session.stage_after(step, question) else {
            return false;
        };
        let running = !matches!(
            stage,
            RoundStage::NoRound | RoundStage::Starting | RoundStage::StartFailed { .. }
        );
        if !running {
            session.asked = 0;
            session.latest_question = None;
        } else if !session.running {
            // A running stage after none starts the session's next round.
            session.rounds += 1;
        }
        session.running = running;
        session.publish(stage);
        true
    }

    /// The rounds the reviewer started from the page of the session behind `token`, or `None`
    /// when no session has that token.
    pub(crate) fn starts(&self, token: &str) -> Option<Vec<SentStart>> {
        find(&mut self.lock(), token).map(|session| session.starts.clone())
    }

    /// The answers the reviewer sent from the page of the session behind `token`, or `None`
    /// when no session has that token.
    pub(crate) fn answers(&self, token: &str) -> Option<Vec<SentAnswer>> {
        find(&mut self.lock(), token).map(|session| session.answers.clone())
    }

    /// Takes a command the page sent for the session behind `token`. The page already refused
    /// an answer to a question its round no longer asks, and a start while a round runs; an
    /// answer that reaches the session is kept, and the agent works on its next turn; a start
    /// is kept, and the round is starting.
    fn command(&self, token: &str, command: PageCommand) -> Result<(), CommandRefusal> {
        let mut sessions = self.lock();
        let session = find(&mut sessions, token).ok_or(CommandRefusal::Stale)?;
        match command {
            PageCommand::Answer(answer) => {
                session.answers.push(SentAnswer {
                    question: answer.question,
                    version: answer.version,
                    choice: answer.input.option,
                    comment: answer.input.text,
                    first_pick: answer.input.first_pick,
                });
                session.publish(RoundStage::AgentWorking);
            }
            PageCommand::DiagramFailed(error) => {
                if !session.diagram_errors.contains(&error) {
                    session.diagram_errors.push(error);
                }
            }
            PageCommand::Start { challenger } => {
                session.starts.push(SentStart { challenger });
                session.publish(RoundStage::Starting);
            }
        }
        Ok(())
    }

    /// The diagram errors the page of the session behind `token` reported, or `None` when no
    /// session has that token.
    pub(crate) fn diagram_errors(&self, token: &str) -> Option<Vec<DiagramError>> {
        find(&mut self.lock(), token).map(|session| session.diagram_errors.clone())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Session>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Rounds for Sessions {
    fn find(&self, token: &str) -> Option<PageRound> {
        let stages = find(&mut self.lock(), token)?.round.subscribe();
        let sessions = self.clone();
        let token = token.to_owned();
        let commands = CommandSender::new(move |command, reply| {
            reply.send(sessions.command(&token, command));
        });
        Some(PageRound::new(stages, commands))
    }
}

fn find<'a>(sessions: &'a mut [Session], token: &str) -> Option<&'a mut Session> {
    sessions
        .iter_mut()
        .find(|session| session.token.matches(token))
}
