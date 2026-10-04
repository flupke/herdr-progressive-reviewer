//! The sessions of the standalone server. Each stands in for the review tool's Explore session:
//! it owns one round, behind its own token, moves it to the step its controller asks for, and
//! takes the answers, the starts, the implementation requests and the quiz answers the reviewer
//! sends from the page.

use std::sync::{Arc, Mutex, PoisonError};

use review_explore::{DiagramError, Question, QuizAnswers, StartBlock};
use review_explore_page::{
    CommandRefusal, CommandSender, ImplementationState, Interruption, LatestAnswer, PageCommand,
    PageImplementation, PageRound, PublishedRound, Recovery, ReviewName, RoundPublisher,
    RoundStage, Rounds, Token,
};
use serde::Serialize;

use crate::fixed_question::{TO_BE_IMPLEMENTED, conclusion_stage, question_stage};
use crate::{fixed_design, fixed_quiz};

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
    /// The lists to be implemented that the reviewer sent from the page, in order.
    implementations: Vec<String>,
    /// What the reviewer answered of the conclusion's quiz, once the agent concluded with one.
    quiz: Option<QuizAnswers>,
    /// The agent's turns the session asked for so far, which name each turn.
    turns: usize,
    /// The reviewer's answers of the latest round that were not cancelled, in the pane or from
    /// the page: the latest one may be cancelled.
    answered: usize,
    /// The other actions the reviewer took on the page, by name, in order.
    actions: Vec<String>,
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
    /// first question, and the reviewer's latest answer, which the reviewer may cancel.
    fn publish(&self, stage: RoundStage) {
        let id = self.round_id();
        let design = fixed_design::design();
        let latest = (self.answered > 0).then(|| LatestAnswer {
            id: format!("answer-{}", self.answered),
            choice: Some(LATEST_CHOICE.into()),
            comment: String::new(),
        });
        let round = self.running.then(|| PublishedRound {
            id: &id,
            design: (self.asked > 0).then_some(&design),
            cancellable: latest.as_ref(),
            earlier: false,
        });
        self.round.publish(round, stage);
    }

    /// Forgets the questions and answers of the round that no longer runs.
    fn forget_round(&mut self) {
        self.asked = 0;
        self.answered = 0;
        self.latest_question = None;
    }

    /// The agent works on the turn that `step` asks for: after the reviewer's answer in the
    /// pane, or the kickoff.
    fn turn_after(&mut self, step: Step) -> RoundStage {
        if matches!(step, Step::Answer) {
            self.answered += 1;
        }
        self.new_turn()
    }

    /// The agent works on a new turn.
    fn new_turn(&mut self) -> RoundStage {
        self.turns += 1;
        self.working()
    }

    /// The agent works on the latest turn.
    fn working(&self) -> RoundStage {
        RoundStage::AgentWorking {
            request: self.turn_id(),
        }
    }

    /// The agent is not working on the latest turn, for `interruption`.
    fn interrupted(&self, interruption: Interruption) -> RoundStage {
        RoundStage::Interrupted {
            request: Some(self.turn_id()),
            interruption,
        }
    }

    fn turn_id(&self) -> String {
        format!("turn-{}", self.turns)
    }

    /// The stage that `step` moves the round to; `question`, when given, is the question the
    /// agent posts instead of the next fixed one. `None` when the round cannot take the step:
    /// the agent asked no question whose answer could be cancelled, or no implementation request
    /// is being sent.
    fn stage_after(&mut self, step: Step, question: Option<Question>) -> Option<RoundStage> {
        Some(match step {
            Step::Question => {
                self.asked += 1;
                let stage = question_stage(self.asked, question);
                self.latest_question = Some(stage.clone());
                stage
            }
            Step::Cancel => self.question_after_cancel_answer()?,
            Step::Answer | Step::Kickoff => self.turn_after(step),
            Step::Fail(failure) => self.prompt_failed(failure),
            Step::Interrupt => self.interrupted(Interruption::Stopped),
            Step::Conclude { quiz } => {
                self.quiz = quiz.then(QuizAnswers::default);
                self.conclusion(None)
            }
            Step::Implement | Step::Deliver => self.implementation_stage(step)?,
            Step::Reset => RoundStage::NoRound,
            Step::FailStart => RoundStage::StartFailed {
                failure: "Repository comparison is not ready; retry Start".into(),
            },
        })
    }

    /// The agent's latest question, asked again once the reviewer cancelled its answer: the
    /// reviewer has seen its recommendation. `None` when the agent asked none.
    fn question_after_cancel_answer(&mut self) -> Option<RoundStage> {
        let mut stage = self.latest_question.clone()?;
        self.answered = self.answered.saturating_sub(1);
        if let RoundStage::Question {
            answer_cancelled, ..
        } = &mut stage
        {
            *answer_cancelled = true;
        }
        Some(stage)
    }

    /// The conclusion with its implementation request once `step` happened to it: the pane
    /// sent one, or the agent received the one the session sends.
    fn implementation_stage(&self, step: Step) -> Option<RoundStage> {
        match step {
            Step::Implement => Some(self.conclusion(Some(PageImplementation {
                delivery: "pane".into(),
                text: TO_BE_IMPLEMENTED.into(),
                state: ImplementationState::Sent,
            }))),
            _ => self.finish_sending(ImplementationState::Sent),
        }
    }

    /// The implementation request of the conclusion the round shows, if any.
    fn implementation(&self) -> Option<PageImplementation> {
        match self.round.subscribe().stage() {
            RoundStage::Conclusion { implementation, .. } => implementation,
            _ => None,
        }
    }

    /// The stage after the prompt the session sends failed with `failure`: the conclusion's
    /// request, while the session sends one, or else the agent's next turn.
    fn prompt_failed(&self, failure: PromptFailure) -> RoundStage {
        self.finish_sending(failure.implementation())
            .unwrap_or_else(|| self.interrupted(Interruption::Failed(failure.reason().into())))
    }

    /// The conclusion's request, which the session is sending, with the outcome `state`.
    fn finish_sending(&self, state: ImplementationState) -> Option<RoundStage> {
        let sending = self
            .implementation()
            .filter(|implementation| implementation.state == ImplementationState::Sending)?;
        Some(self.conclusion(Some(PageImplementation { state, ..sending })))
    }

    /// The fixed conclusion, with `implementation` and, when the agent concluded with one, its
    /// quiz.
    fn conclusion(&self, implementation: Option<PageImplementation>) -> RoundStage {
        conclusion_stage(implementation, self.quiz.clone())
    }
}

/// How a prompt the session sends fails.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PromptFailure {
    /// It could not be delivered.
    NotSent,
    /// The agent did not start on it.
    NotStarted,
}

impl PromptFailure {
    /// What becomes of the conclusion's implementation request.
    fn implementation(self) -> ImplementationState {
        match self {
            Self::NotSent => ImplementationState::NotSent(self.reason().into()),
            Self::NotStarted => ImplementationState::NotStarted,
        }
    }

    /// Why the agent's next turn stopped, as the review tool says it.
    fn reason(self) -> &'static str {
        match self {
            Self::NotSent => "The selected agent is no longer available",
            Self::NotStarted => {
                "The agent did not start on the prompt. Look at the agent's pane, then Retry."
            }
        }
    }
}

impl Session {
    /// Takes one of the page's actions that recover or close the round, as the review tool
    /// would. The page already refused one its round no longer offers.
    fn recover(&mut self, recovery: &Recovery) -> Result<(), CommandRefusal> {
        let (name, stage) = match recovery {
            Recovery::Stop { request: None } => ("stop", Some(RoundStage::NoRound)),
            Recovery::Stop { request: Some(_) } => {
                ("stop", Some(self.interrupted(Interruption::Stopped)))
            }
            Recovery::Retry { .. } => ("retry", Some(self.working())),
            Recovery::CancelAnswer { .. } => ("cancel-answer", self.question_after_cancel_answer()),
            Recovery::Reset { .. } => ("reset", Some(RoundStage::NoRound)),
            Recovery::CancelImplementation { .. } => (
                "cancel-implementation",
                self.finish_sending(ImplementationState::Cancelled),
            ),
            Recovery::ResendImplementation { .. } => {
                ("resend-implementation", Some(self.resend_implementation()))
            }
        };
        let stage = stage.ok_or(CommandRefusal::Stale)?;
        if matches!(stage, RoundStage::NoRound) {
            self.running = false;
            self.forget_round();
        }
        self.actions.push(name.into());
        self.publish(stage);
        Ok(())
    }

    /// The conclusion with its implementation request sent again, as it is.
    fn resend_implementation(&self) -> RoundStage {
        let implementation = self
            .implementation()
            .map(|implementation| PageImplementation {
                state: ImplementationState::Sending,
                ..implementation
            });
        self.conclusion(implementation)
    }
}

/// The choice of the reviewer's latest answer, as the page shows it to cancel it.
const LATEST_CHOICE: &str = "Keep the draft";

/// What happens next in a session's round.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    /// The agent posts its next question.
    Question,
    /// The reviewer answered in the pane: the agent works on its next turn.
    Answer,
    /// The prompt the session sends fails: the conclusion's implementation request, while the
    /// session sends one, or else the agent's next turn.
    Fail(PromptFailure),
    /// The reviewer cancels the latest answer in the pane: its question waits again.
    Cancel,
    /// The agent stops before its next turn.
    Interrupt,
    /// The agent concludes the round, with a quiz when `quiz`.
    Conclude { quiz: bool },
    /// The reviewer implements the conclusion in the pane, and the agent receives the request.
    Implement,
    /// The agent receives the conclusion's implementation request that the session sends.
    Deliver,
    /// The reviewer resets the round: no round is running.
    Reset,
    /// The tool sent the kickoff of the round the reviewer started: the agent works on its first
    /// turn.
    Kickoff,
    /// The round the reviewer started could not start: no round is running.
    FailStart,
}

impl Step {
    /// Each step by the name of its control route.
    const NAMES: [(&str, Self); 13] = [
        ("question", Self::Question),
        ("answer", Self::Answer),
        ("fail", Self::Fail(PromptFailure::NotSent)),
        ("not-started", Self::Fail(PromptFailure::NotStarted)),
        ("cancel", Self::Cancel),
        ("interrupt", Self::Interrupt),
        ("conclude", Self::Conclude { quiz: false }),
        ("conclude-with-quiz", Self::Conclude { quiz: true }),
        ("implement", Self::Implement),
        ("deliver", Self::Deliver),
        ("reset", Self::Reset),
        ("kickoff", Self::Kickoff),
        ("fail-start", Self::FailStart),
    ];

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::NAMES
            .into_iter()
            .find_map(|(known, step)| (known == name).then_some(step))
    }
}

impl Sessions {
    /// Opens a session behind `token` whose agent asked `asked` questions: it shows the latest,
    /// or the agent works on its first one.
    pub(crate) fn open(&self, token: Token, asked: usize) {
        let latest_question = (asked > 0).then(|| question_stage(asked, None));
        let stage = latest_question.clone().unwrap_or(RoundStage::AgentWorking {
            request: "turn-0".into(),
        });
        let round = RoundPublisher::default();
        round.name(ReviewName {
            repository: "drafts-demo".into(),
            revision: "kmzqvtyx".into(),
            title: "Keep the reviewer's draft when a round reopens".into(),
        });
        let session = Session {
            token,
            round,
            rounds: 1,
            running: true,
            asked,
            latest_question,
            answers: Vec::new(),
            diagram_errors: Vec::new(),
            starts: Vec::new(),
            implementations: Vec::new(),
            quiz: None,
            turns: 0,
            answered: 0,
            actions: Vec::new(),
        };
        session.publish(stage);
        self.lock().push(session);
    }

    /// Moves the round of the session behind `token` one step; `question`, when given, is the
    /// question the agent posts instead of the next fixed one. Returns false when no session
    /// has that token, or when the round cannot take the step.
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
            session.forget_round();
        } else if !session.running {
            // A running stage after none starts the session's next round.
            session.rounds += 1;
        }
        session.running = running;
        session.publish(stage);
        true
    }

    /// The reviewer changes the review marks of the session behind `token`, so that `block` says
    /// why no round can start, or with `None` that one can. The round stays where it is. Returns
    /// false when no session has that token.
    pub(crate) fn block_starts(&self, token: &str, block: Option<StartBlock>) -> bool {
        let mut sessions = self.lock();
        let Some(session) = find(&mut sessions, token) else {
            return false;
        };
        session.round.block_starts(block);
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

    /// The lists to be implemented that the reviewer sent from the page of the session behind
    /// `token`, or `None` when no session has that token.
    pub(crate) fn implementations(&self, token: &str) -> Option<Vec<String>> {
        find(&mut self.lock(), token).map(|session| session.implementations.clone())
    }

    /// Takes a command the page sent for the session behind `token`. The page already refused
    /// an answer to a question its round no longer asks, a start while a round runs, and an
    /// Implement its conclusion no longer offers. An answer that reaches the session is kept,
    /// and the agent works on its next turn; a start is kept, and the round is starting; an
    /// implementation request is kept, and the session sends it.
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
                session.answered += 1;
                let stage = session.new_turn();
                session.publish(stage);
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
            PageCommand::Implement(implement) => {
                session.implementations.push(implement.text.clone());
                let sending = PageImplementation {
                    delivery: format!("page-{}", session.implementations.len()),
                    text: implement.text,
                    state: ImplementationState::Sending,
                };
                let stage = session.conclusion(Some(sending));
                session.publish(stage);
            }
            PageCommand::Quiz(quiz) => {
                let answers = session.quiz.as_mut().ok_or(CommandRefusal::Stale)?;
                answers
                    .record(&fixed_quiz::items(), quiz.response)
                    .map_err(|_| CommandRefusal::Stale)?;
                let stage = session.conclusion(session.implementation());
                session.publish(stage);
            }
            PageCommand::Reply(_) => {
                session.actions.push("reply".into());
                let stage = session.new_turn();
                session.publish(stage);
            }
            PageCommand::Recover(recovery) => session.recover(&recovery)?,
        }
        Ok(())
    }

    /// The other actions the reviewer took on the page of the session behind `token`, by name,
    /// in order, or `None` when no session has that token.
    pub(crate) fn actions(&self, token: &str) -> Option<Vec<String>> {
        find(&mut self.lock(), token).map(|session| session.actions.clone())
    }

    /// What the reviewer answered of the quiz of the session behind `token`, nothing when its
    /// round has no quiz, or `None` when no session has that token.
    pub(crate) fn quiz(&self, token: &str) -> Option<QuizAnswers> {
        find(&mut self.lock(), token).map(|session| session.quiz.clone().unwrap_or_default())
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
