//! The sessions of the standalone server. Each stands in for the review tool's Explore session:
//! it owns one round, behind its own token, moves it to the step its controller asks for, and
//! takes the answers, the starts, the implementation requests and the quiz answers the reviewer
//! sends from the page.

use std::sync::{Arc, Mutex, PoisonError};

use review_explore::{
    AgentRecord, Decision, DiagramError, EarlierQuestion, KeptAnswer, MarkCounts, Question,
    QuizAnswers, QuizProgress, QuizResponse, RoundOverview, StartBlock, TurnMarks,
};
use review_explore_citations::Citation;
use review_explore_page::{
    Answered, AnsweredQuestion, CommandRefusal, CommandSender, ImplementationState, Interruption,
    LatestAnswer, PageCommand, PageConversation, PageImplementation, PageRound, PublishedRound,
    QuestionMarks, Recovery, RoundPublisher, RoundStage, Rounds, SentAnswer, ThreadSender, Token,
    Waiting,
};
use review_explore_tally::MarkTally;
use review_threads::ThreadCommand;
use serde::Serialize;

use crate::conversation::{SentMessage, SessionThreads};
use crate::overview::Posted;
use crate::round_data::RoundData;
use crate::tally::SessionMarks;

#[derive(Clone)]
pub(crate) struct Sessions {
    /// What the agent of every session posts.
    data: &'static dyn RoundData,
    /// The clock of every session.
    clock: Clock,
    open: Arc<Mutex<Vec<Session>>>,
}

struct Session {
    token: Token,
    /// When things happen: when the reviewer starts a round, and when a turn goes out.
    clock: Clock,
    /// The reviewer's answer the agent's latest turn carries, if any.
    turn_answer: Option<Box<SentAnswer>>,
    /// When the latest attempt of the agent's latest turn went out, in milliseconds since the
    /// epoch.
    sent_at_ms: Option<u64>,
    /// Names the session's rounds, so that no two sessions' rounds have the same identity.
    id: String,
    /// What the agent posts.
    data: &'static dyn RoundData,
    round: RoundPublisher,
    /// The rounds the session started so far.
    rounds: usize,
    /// Whether the latest round is running: the reviewer has not reset it.
    running: bool,
    /// The questions the agent asked so far.
    asked: usize,
    /// The stage of the agent's latest question, which a cancelled answer brings back.
    latest_question: Option<RoundStage>,
    /// Whether the agent's latest turn posted the conclusion.
    concluded: bool,
    /// The answers the reviewer sent from the page, in order.
    answers: Vec<PostedAnswer>,
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
    /// The attempts to reach the agent so far, of a turn or an implementation request, which
    /// name each attempt.
    attempts: usize,
    /// The starts the session offered so far, which name each start.
    offers: usize,
    /// The reviewer's answers of the latest round that were not cancelled, in the pane or from
    /// the page, in order, as the page shows them: the latest one may be cancelled.
    answered: Vec<LatestAnswer>,
    /// The same answers, as the conclusion lists them in "Your decisions".
    decisions: Vec<Decision>,
    /// The questions of the same answers, as the rail opens them once the agent took each
    /// answer up, with their citations.
    answered_questions: Vec<(EarlierQuestion, Arc<[Citation]>)>,
    /// How many of `answered_questions` the agent took up: it posted its next question or the
    /// conclusion after them, so they are done steps of the rail.
    taken_up: usize,
    /// The lines each of the same answers marked.
    answer_marks: Vec<QuestionMarks>,
    /// The lines the reviewer marked by hand in the change.
    marked_by_hand: u64,
    /// The other actions the reviewer took on the page, by name, in order.
    actions: Vec<String>,
    /// The review threads, which hold the conversation of each of the session's rounds.
    threads: SessionThreads,
    /// Whether another reviewer saved a newer round of the review since: the reviewer can only
    /// Reset this one.
    earlier: bool,
    /// How the session's page follows the round.
    page: PageLink,
}

/// The clock of the sessions: the time now, or always the same time, so that two gallery runs
/// draw the same page.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Clock {
    Real,
    Fixed,
}

impl Clock {
    /// The time now by this clock, in milliseconds since the epoch.
    fn now_ms(self) -> u64 {
        match self {
            Self::Real => review_explore::now_ms(),
            Self::Fixed => FIXED_NOW_MS,
        }
    }
}

/// The time of a fixed clock, in milliseconds since the epoch: the gallery's pages stand 42
/// seconds after it (`tests/explore-page/gallery/gallery.ts`).
const FIXED_NOW_MS: u64 = 1_791_000_000_000;

/// How a session's page follows its round.
#[derive(Default)]
enum PageLink {
    /// It shows each change.
    #[default]
    Following,
    /// It no longer follows the round, as a page whose socket does not get the tool's messages,
    /// until it sends an action; the round may have moved to `unseen` meanwhile.
    Held { unseen: Option<Box<RoundStage>> },
    /// The reviewer is restarting: the page cannot open its socket.
    Away,
}

/// An answer the reviewer sent from the page, as a test reads it back.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct PostedAnswer {
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
        format!("{}-{}", self.id, self.rounds)
    }

    /// Publishes `stage` of the latest round, with the design once the agent asked its first
    /// question, and the reviewer's latest answer, which the reviewer may cancel.
    fn publish(&mut self, stage: RoundStage) {
        let id = self.round_id();
        let design = self.data.design();
        let items = self.data.quiz_items().len();
        let (earlier, earlier_citations): (Vec<_>, Vec<_>) = self.answered_questions
            [..self.taken_up]
            .iter()
            .cloned()
            .unzip();
        let overview = RoundOverview {
            decisions: self.decisions.clone(),
            earlier,
            ..Posted {
                asked: self.asked,
                concluded: self.concluded,
                quiz: self.quiz.as_ref().map(|answers| QuizProgress {
                    items,
                    answers: Some(answers),
                }),
            }
            .overview(&stage)
        };
        let review = SessionThreads::review();
        let round = self.running.then(|| PublishedRound {
            id: &id,
            review_unit: &review,
            design: (self.asked > 0).then_some(&design),
            changed_files: self.data.change().0.len(),
            cancellable: self.answered.last(),
            earlier: self.earlier,
            overview: &overview,
            earlier_citations: &earlier_citations,
        });
        if let PageLink::Held { unseen } = &mut self.page {
            *unseen = Some(Box::new(stage));
        } else {
            let tally = self.tally(&stage);
            self.round.publish_counted(round, stage, tally);
        }
    }

    /// How much of the change the review marks cover while the round is at `stage`.
    fn tally(&self, stage: &RoundStage) -> MarkTally {
        let waiting = match stage {
            RoundStage::Question {
                question, marks, ..
            } if self.running && !self.earlier => Some((&**question, marks)),
            _ => None,
        };
        SessionMarks {
            change: self.data.change(),
            jev: self.data.jev_marks(),
            running: self.running,
            answered: &self.answer_marks,
            by_hand: self.marked_by_hand,
            waiting,
        }
        .tally()
    }

    /// The stage the round is at, which a held page may not show yet.
    fn stage(&self) -> RoundStage {
        match &self.page {
            PageLink::Held {
                unseen: Some(stage),
            } => (**stage).clone(),
            _ => self.round.subscribe().stage(),
        }
    }

    /// The page follows the round again: it shows the stage the round moved to meanwhile.
    fn release(&mut self) {
        if let PageLink::Held { unseen } = std::mem::take(&mut self.page)
            && let Some(stage) = unseen
        {
            self.publish(*stage);
        }
    }

    /// The reviewer restarts: the page's socket closes, and the page cannot open another one
    /// until the reviewer is back. The round stays where it is.
    fn restart(&mut self) {
        let stage = self.stage();
        let feed = self.round.subscribe();
        let round = RoundPublisher::default();
        if let Some(review) = feed.review() {
            round.name(review);
        }
        round.block_starts(feed.start_block());
        self.round = round;
        self.page = PageLink::Away;
        self.publish(stage);
    }

    /// Takes the reviewer's answer to the agent's latest question, and returns it as the agent's
    /// next turn carries it.
    fn take_answer(&mut self, answer: AnswerTaken<'_>) -> SentAnswer {
        let (alternatives, question) = match &self.latest_question {
            Some(RoundStage::Question { question, .. }) => (
                question.alternatives.as_slice(),
                Some((question.id.clone(), question.version)),
            ),
            _ => (&[][..], None),
        };
        let (picked, comment, first_pick) = match answer {
            AnswerTaken::InPane => (
                alternatives.first(),
                self.data.pane_comment().to_owned(),
                None,
            ),
            AnswerTaken::AfterFirstPick => {
                let kept = alternatives
                    .iter()
                    .find(|alternative| alternative.recommendation.is_some())
                    .or(alternatives.first());
                let first = alternatives.iter().find(|alternative| {
                    Some(alternative.id.as_str()) != kept.map(|kept| kept.id.as_str())
                });
                (kept, String::new(), first.map(|first| first.id.as_str()))
            }
            AnswerTaken::FromPage {
                choice,
                comment,
                first_pick,
            } => (
                alternatives
                    .iter()
                    .find(|alternative| Some(alternative.id.as_str()) == choice),
                comment.to_owned(),
                first_pick,
            ),
        };
        let kept = KeptAnswer::new(picked, &comment, first_pick);
        let mut turn_answer = SentAnswer {
            question: None,
            kept: kept.clone(),
            marked: MarkCounts::default(),
        };
        if let Some(RoundStage::Question {
            question,
            citations,
            marks,
            ..
        }) = &self.latest_question
        {
            let number = self.decisions.len() + 1;
            self.decisions.push(Decision {
                number,
                question: question.text.clone(),
                answer: kept.clone(),
            });
            let record = EarlierQuestion {
                number,
                question: (**question).clone(),
                answer: Some(kept),
                // What the agent records, once it takes the answer up.
                recorded: AgentRecord::default(),
                marks: vec![TurnMarks {
                    reviewed: marks.reviewed.clone(),
                    not_relevant: marks.not_relevant.clone(),
                    ..TurnMarks::default()
                }],
            };
            self.answered_questions.push((record, citations.clone()));
            turn_answer.question = Some(AnsweredQuestion {
                question: question.clone(),
                citations: citations.clone(),
                picked_blind: first_pick.is_some(),
            });
            turn_answer.marked = marks.counts();
        }
        self.answer_marks.push(match &self.latest_question {
            Some(RoundStage::Question { marks, .. }) => marks.clone(),
            _ => QuestionMarks::default(),
        });
        self.answered.push(LatestAnswer {
            id: format!("answer-{}", self.answered.len() + 1),
            choice: picked.map(|alternative| alternative.text.clone()),
            comment,
            answered: Answered {
                question,
                option: picked.map(|alternative| alternative.id.clone()),
                in_reply_to: self.turn_id(),
            },
        });
        turn_answer
    }

    /// The agent's turn that posted `stage` took up the answers it follows: their questions
    /// are done steps of the rail, with what the agent recorded of each.
    fn take_up_answers(&mut self, stage: &RoundStage) {
        let response = stage.response();
        for (record, _) in &mut self.answered_questions[self.taken_up..] {
            record.recorded = AgentRecord {
                interpretation: response
                    .and_then(|response| response.interpretations.last().cloned()),
                reply: response.and_then(|response| response.reply.clone()),
            };
        }
        self.taken_up = self.answered_questions.len();
    }

    /// Forgets the questions and answers of the round that no longer runs.
    fn forget_round(&mut self) {
        self.asked = 0;
        self.answered.clear();
        self.decisions.clear();
        self.answered_questions.clear();
        self.taken_up = 0;
        self.answer_marks.clear();
        self.latest_question = None;
        self.turn_answer = None;
        self.concluded = false;
        self.earlier = false;
    }

    /// The agent works on the turn that `step` asks for: after the reviewer's answer in the
    /// pane, or the kickoff.
    fn turn_after(&mut self, step: Step) -> RoundStage {
        let answer = match step {
            Step::Answer {
                after_first_pick: false,
            } => Some(self.take_answer(AnswerTaken::InPane)),
            Step::Answer {
                after_first_pick: true,
            } => Some(self.take_answer(AnswerTaken::AfterFirstPick)),
            _ => None,
        };
        self.new_turn(answer)
    }

    /// The agent works on a new turn, which carries the reviewer's `answer`, if any.
    fn new_turn(&mut self, answer: Option<SentAnswer>) -> RoundStage {
        self.turns += 1;
        self.turn_answer = answer.map(Box::new);
        self.attempt_again()
    }

    /// The latest turn goes out to the agent again, which works on it.
    fn attempt_again(&mut self) -> RoundStage {
        self.attempts += 1;
        self.sent_at_ms = Some(self.clock.now_ms());
        self.working()
    }

    /// The identity of the latest attempt to reach the agent.
    fn attempt(&self) -> String {
        format!("attempt-{}", self.attempts)
    }

    /// The start the session offers while no round runs.
    fn offered_start(&self) -> String {
        format!("{}-start-{}", self.id, self.offers)
    }

    /// No round is running: the session offers its next start.
    fn no_round(&mut self) -> RoundStage {
        self.offers += 1;
        RoundStage::NoRound {
            start: self.offered_start(),
        }
    }

    /// The agent works on the latest turn.
    fn working(&self) -> RoundStage {
        RoundStage::AgentWorking {
            request: self.turn_id(),
            sent_at_ms: self.sent_at_ms,
            answer: self.turn_answer.clone(),
        }
    }

    /// The agent is not working on the latest turn, for `interruption`.
    fn interrupted(&self, interruption: Interruption) -> RoundStage {
        RoundStage::Interrupted {
            request: Some(self.turn_id()),
            attempt: Some(self.attempt()),
            interruption,
            answer: self.turn_answer.clone(),
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
                self.concluded = false;
                let stage = self.data.question_stage(self.asked, question);
                self.latest_question = Some(stage.clone());
                self.take_up_answers(&stage);
                stage
            }
            Step::Cancel => self.question_after_cancel_answer()?,
            Step::Answer { .. } | Step::Kickoff => self.turn_after(step),
            Step::Prompt(outcome) => self.prompt_unworked(outcome),
            Step::Interrupt => self.interrupted(Interruption::Stopped),
            Step::Conclude { quiz } => {
                self.concluded = true;
                self.quiz = quiz.then(QuizAnswers::default);
                let stage = self.conclusion(None);
                self.take_up_answers(&stage);
                stage
            }
            Step::Implement | Step::Deliver => self.implementation_stage(step)?,
            Step::Round(event) => self.after_round_event(event),
        })
    }

    /// The stage after `event`, outside the agent's turns.
    fn after_round_event(&mut self, event: RoundEvent) -> RoundStage {
        match event {
            RoundEvent::Reset => self.no_round(),
            RoundEvent::FailStart => {
                self.offers += 1;
                RoundStage::StartFailed {
                    failure: self.round.subscribe().start_block().map_or_else(
                        || "Repository comparison is not ready; retry Start".into(),
                        |block| block.reason().into(),
                    ),
                    start: self.offered_start(),
                }
            }
            RoundEvent::Earlier => {
                self.earlier = true;
                self.stage()
            }
            RoundEvent::FailStorage => RoundStage::StorageFailed {
                failure: STORAGE_FAILURE.into(),
            },
        }
    }

    /// The agent's latest question, asked again once the reviewer cancelled its answer: the
    /// reviewer has seen its recommendation. `None` when the agent asked none.
    fn question_after_cancel_answer(&mut self) -> Option<RoundStage> {
        let mut stage = self.latest_question.clone()?;
        self.answered.pop();
        self.concluded = false;
        self.decisions.pop();
        self.answered_questions.pop();
        self.taken_up = self.taken_up.min(self.answered_questions.len());
        self.answer_marks.pop();
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
                attempt: self.attempt(),
                text: self.data.to_be_implemented(),
                state: ImplementationState::Sent,
                sent_at_ms: Some(SENT_AT_MS),
            }))),
            _ => self.finish_sending(ImplementationState::Sent),
        }
    }

    /// The implementation request of the conclusion the round shows, if any.
    fn implementation(&self) -> Option<PageImplementation> {
        match self.stage() {
            RoundStage::Conclusion { implementation, .. } => implementation,
            _ => None,
        }
    }

    /// The stage once the prompt the session sends ended as `outcome`, without the agent working
    /// on it: the conclusion's request, while the session sends one, or else the agent's next
    /// turn.
    fn prompt_unworked(&self, outcome: PromptOutcome) -> RoundStage {
        self.finish_sending(outcome.implementation())
            .unwrap_or_else(|| self.interrupted(outcome.interruption()))
    }

    /// The conclusion's request, which the session is sending, with the outcome `state`.
    fn finish_sending(&self, state: ImplementationState) -> Option<RoundStage> {
        let sending = self
            .implementation()
            .filter(|implementation| implementation.state == ImplementationState::Sending)?;
        let sent_at_ms = (state == ImplementationState::Sent).then_some(SENT_AT_MS);
        Some(self.conclusion(Some(PageImplementation {
            state,
            sent_at_ms,
            ..sending
        })))
    }

    /// The fixed conclusion, with `implementation` and, when the agent concluded with one, its
    /// quiz.
    fn conclusion(&self, implementation: Option<PageImplementation>) -> RoundStage {
        self.data
            .conclusion_stage(implementation, self.quiz.clone())
    }
}

/// An answer the reviewer gives to the agent's latest question.
#[derive(Clone, Copy)]
enum AnswerTaken<'a> {
    /// In the pane, which picks the first choice, with no comment.
    InPane,
    /// The recommended choice, after a first pick of another, with no comment.
    AfterFirstPick,
    /// From the page: the choice picked, by ID, if any, the comment, and the choice picked
    /// first on a blind question.
    FromPage {
        choice: Option<&'a str>,
        comment: &'a str,
        first_pick: Option<&'a str>,
    },
}

/// How a prompt the session sends ends without the agent working on it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PromptOutcome {
    /// It could not be delivered.
    NotSent,
    /// The agent did not start on it.
    NotStarted,
    /// The reviewer reopened the review before the prompt was sent.
    ReopenedBeforeSending,
    /// The reviewer reopened the review while the prompt was being delivered: whether the agent
    /// received it is unknown.
    ReopenedWhileSending,
}

impl PromptOutcome {
    /// What becomes of the conclusion's implementation request.
    fn implementation(self) -> ImplementationState {
        match self {
            Self::NotSent => ImplementationState::NotSent(NOT_DELIVERED.into()),
            Self::NotStarted => ImplementationState::NotStarted,
            Self::ReopenedBeforeSending => ImplementationState::Paused,
            Self::ReopenedWhileSending => ImplementationState::Unknown,
        }
    }

    /// Why the agent is not working on its next turn.
    fn interruption(self) -> Interruption {
        match self {
            Self::NotSent => Interruption::Failed(NOT_DELIVERED.into()),
            Self::NotStarted => Interruption::NotStarted,
            Self::ReopenedBeforeSending => Interruption::Stopped,
            Self::ReopenedWhileSending => Interruption::Uncertain,
        }
    }
}

/// When the standalone agent receives an implementation request, in milliseconds since the
/// epoch: always the same time, so that two gallery runs draw the same page.
const SENT_AT_MS: u64 = 1_790_000_000_000;

/// Why the standalone server's prompts cannot be delivered.
const NOT_DELIVERED: &str = "The selected agent is no longer available";

/// The lines the reviewer marks by hand at a time.
const HAND_MARKED_LINES: u64 = 3;

/// Why the standalone server cannot save the reviewer's rounds.
const STORAGE_FAILURE: &str = "No space left on device (os error 28)";

impl Session {
    /// Takes one of the page's actions that recover or close the round, as the review tool
    /// would. The page already refused one its round no longer offers.
    fn recover(&mut self, recovery: &Recovery) -> Result<(), CommandRefusal> {
        let (name, stage) = match recovery {
            Recovery::Stop(Waiting::Start(_)) => ("stop", Some(self.no_round())),
            Recovery::Stop(Waiting::Turn(_)) => {
                ("stop", Some(self.interrupted(Interruption::Stopped)))
            }
            Recovery::Retry { .. } => ("retry", Some(self.attempt_again())),
            Recovery::CancelAnswer { .. } => ("cancel-answer", self.question_after_cancel_answer()),
            Recovery::Reset { .. } => ("reset", Some(self.no_round())),
            Recovery::CancelImplementation { .. } => (
                "cancel-implementation",
                self.finish_sending(ImplementationState::Cancelled),
            ),
            Recovery::ResendImplementation { .. } => {
                ("resend-implementation", Some(self.resend_implementation()))
            }
        };
        let stage = stage.ok_or(CommandRefusal::Stale)?;
        if matches!(stage, RoundStage::NoRound { .. }) {
            self.running = false;
            self.forget_round();
        }
        self.actions.push(name.into());
        self.publish(stage);
        Ok(())
    }

    /// The conclusion with its implementation request sent again, as it is.
    fn resend_implementation(&mut self) -> RoundStage {
        self.attempts += 1;
        let attempt = self.attempt();
        let implementation = self
            .implementation()
            .map(|implementation| PageImplementation {
                state: ImplementationState::Sending,
                attempt,
                ..implementation
            });
        self.conclusion(implementation)
    }
}

impl Session {
    /// Takes a command the page sent, which the page checked against the round already.
    fn take(&mut self, command: PageCommand) -> Result<(), CommandRefusal> {
        match command {
            PageCommand::Answer(answer) => {
                let sent = self.take_answer(AnswerTaken::FromPage {
                    choice: answer.input.option.as_deref(),
                    comment: &answer.input.text,
                    first_pick: answer.input.first_pick.as_deref(),
                });
                self.answers.push(PostedAnswer {
                    question: answer.question,
                    version: answer.version,
                    choice: answer.input.option,
                    comment: answer.input.text,
                    first_pick: answer.input.first_pick,
                });
                let stage = self.new_turn(Some(sent));
                self.publish(stage);
            }
            // The page keeps the pick; the round still asks the question, which the page checked.
            PageCommand::Pick { .. } => {}
            PageCommand::DiagramFailed(error) => {
                if !self.diagram_errors.contains(&error) {
                    self.diagram_errors.push(error);
                }
            }
            PageCommand::Start { challenger, start } => {
                self.starts.push(SentStart { challenger });
                self.publish(RoundStage::Starting {
                    start,
                    started_at_ms: Some(self.clock.now_ms()),
                });
            }
            PageCommand::Implement(implement) => {
                self.implementations.push(implement.text.clone());
                self.attempts += 1;
                let sending = PageImplementation {
                    delivery: format!("page-{}", self.implementations.len()),
                    attempt: self.attempt(),
                    text: implement.text,
                    state: ImplementationState::Sending,
                    sent_at_ms: None,
                };
                let stage = self.conclusion(Some(sending));
                self.publish(stage);
            }
            PageCommand::Quiz(quiz) => self.take_quiz(quiz.response)?,
            PageCommand::Recover(recovery) => self.recover(&recovery)?,
        }
        Ok(())
    }

    /// Saves the reviewer's quiz `response`.
    fn take_quiz(&mut self, response: QuizResponse) -> Result<(), CommandRefusal> {
        let answers = self.quiz.as_mut().ok_or(CommandRefusal::Stale)?;
        answers
            .record(&self.data.quiz_items(), response)
            .map_err(|_| CommandRefusal::Stale)?;
        let stage = self.conclusion(self.implementation());
        self.publish(stage);
        Ok(())
    }
}

/// What happens to the page of a session, outside its round.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PageChange {
    /// The page no longer follows the round, until it sends an action.
    Hold,
    /// The reviewer restarts: the page's socket closes, and it cannot open another one.
    Restart,
    /// The reviewer is back: the page opens its socket again.
    Back,
}

/// What happens next in a session's round.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    /// The agent posts its next question.
    Question,
    /// The reviewer answered: the agent works on its next turn. The answer keeps the
    /// question's first choice, as one in the pane does; or, `after_first_pick`, the choice the
    /// agent recommends (its first choice when it recommends none) after a first pick of another
    /// choice, as a blind question on the page records it.
    Answer { after_first_pick: bool },
    /// The prompt the session sends ends without the agent working on it: the conclusion's
    /// implementation request, while the session sends one, or else the agent's next turn.
    Prompt(PromptOutcome),
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
    /// The tool sent the kickoff of the round the reviewer started: the agent works on its first
    /// turn.
    Kickoff,
    /// The reviewer or the review tool changes the round outside the agent's turns.
    Round(RoundEvent),
}

/// What the reviewer or the review tool does to a session's round outside the agent's turns.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RoundEvent {
    /// The reviewer resets the round: no round is running.
    Reset,
    /// The round the reviewer started could not start: no round is running.
    FailStart,
    /// Another reviewer saved a newer round of the review: this one stays where it is, and
    /// offers only Reset.
    Earlier,
    /// The review tool cannot save the reviewer's rounds any more.
    FailStorage,
}

impl Step {
    /// Each step by the name of its control route.
    const NAMES: [(&str, Self); 18] = [
        ("question", Self::Question),
        (
            "answer",
            Self::Answer {
                after_first_pick: false,
            },
        ),
        (
            "answer-after-first-pick",
            Self::Answer {
                after_first_pick: true,
            },
        ),
        ("fail", Self::Prompt(PromptOutcome::NotSent)),
        ("not-started", Self::Prompt(PromptOutcome::NotStarted)),
        (
            "reopen-unsent",
            Self::Prompt(PromptOutcome::ReopenedBeforeSending),
        ),
        (
            "reopen-sending",
            Self::Prompt(PromptOutcome::ReopenedWhileSending),
        ),
        ("cancel", Self::Cancel),
        ("interrupt", Self::Interrupt),
        ("conclude", Self::Conclude { quiz: false }),
        ("conclude-with-quiz", Self::Conclude { quiz: true }),
        ("implement", Self::Implement),
        ("deliver", Self::Deliver),
        ("reset", Self::Round(RoundEvent::Reset)),
        ("kickoff", Self::Kickoff),
        ("fail-start", Self::Round(RoundEvent::FailStart)),
        ("earlier", Self::Round(RoundEvent::Earlier)),
        ("fail-storage", Self::Round(RoundEvent::FailStorage)),
    ];

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::NAMES
            .into_iter()
            .find_map(|(known, step)| (known == name).then_some(step))
    }
}

impl Sessions {
    /// No session yet; the agent of each posts `data`.
    pub(crate) fn new(data: &'static dyn RoundData, clock: Clock) -> Self {
        Self {
            data,
            clock,
            open: Arc::default(),
        }
    }

    /// Opens a session behind `token` whose agent asked `asked` questions: it shows the latest,
    /// or the agent works on its first one.
    pub(crate) fn open(&self, token: Token, asked: usize) {
        let latest_question = (asked > 0).then(|| self.data.question_stage(asked, None));
        let sent_at_ms = Some(self.clock.now_ms());
        let stage = latest_question.clone().unwrap_or(RoundStage::AgentWorking {
            request: "turn-0".into(),
            sent_at_ms,
            answer: None,
        });
        let round = RoundPublisher::default();
        round.name(self.data.review());
        let mut session = Session {
            id: format!("session-{}", self.lock().len() + 1),
            token,
            clock: self.clock,
            turn_answer: None,
            sent_at_ms,
            data: self.data,
            round,
            rounds: 1,
            running: true,
            asked,
            latest_question,
            concluded: false,
            answers: Vec::new(),
            diagram_errors: Vec::new(),
            starts: Vec::new(),
            implementations: Vec::new(),
            quiz: None,
            turns: 0,
            attempts: 0,
            offers: 0,
            answered: Vec::new(),
            decisions: Vec::new(),
            answered_questions: Vec::new(),
            taken_up: 0,
            answer_marks: Vec::new(),
            marked_by_hand: 0,
            actions: Vec::new(),
            threads: SessionThreads::new(),
            earlier: false,
            page: PageLink::Following,
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
            RoundStage::NoRound { .. }
                | RoundStage::Starting { .. }
                | RoundStage::StartFailed { .. }
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

    /// The reviewer marks lines of the change by hand, in the session behind `token`: the
    /// page hears the marks at once, whatever the round's stage. Returns false when no session
    /// has that token.
    pub(crate) fn mark_by_hand(&self, token: &str) -> bool {
        let mut sessions = self.lock();
        let Some(session) = find(&mut sessions, token) else {
            return false;
        };
        session.marked_by_hand += HAND_MARKED_LINES;
        // A held page hears the marks with the round's next stage.
        if !matches!(session.page, PageLink::Held { .. }) {
            session.round.tally(session.tally(&session.stage()));
        }
        true
    }

    /// Holds the page of the session behind `token`, restarts its reviewer, or brings the
    /// reviewer back, as `change` says. Returns false when no session has that token.
    pub(crate) fn change_page(&self, token: &str, change: PageChange) -> bool {
        let mut sessions = self.lock();
        let Some(session) = find(&mut sessions, token) else {
            return false;
        };
        match change {
            PageChange::Hold => session.page = PageLink::Held { unseen: None },
            PageChange::Restart => session.restart(),
            PageChange::Back => session.page = PageLink::Following,
        }
        true
    }

    /// Whether `token` opens a session whose reviewer is restarting.
    pub(crate) fn away(&self, token: &str) -> bool {
        find(&mut self.lock(), token).is_some_and(|session| matches!(session.page, PageLink::Away))
    }

    /// The rounds the reviewer started from the page of the session behind `token`, or `None`
    /// when no session has that token.
    pub(crate) fn starts(&self, token: &str) -> Option<Vec<SentStart>> {
        find(&mut self.lock(), token).map(|session| session.starts.clone())
    }

    /// The answers the reviewer sent from the page of the session behind `token`, or `None`
    /// when no session has that token.
    pub(crate) fn answers(&self, token: &str) -> Option<Vec<PostedAnswer>> {
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
        let held = matches!(session.page, PageLink::Held { .. });
        if held && !matches!(command, PageCommand::DiagramFailed(_)) {
            // The page acted on the round as it showed it, which the round moved past.
            session.release();
            return Err(CommandRefusal::Stale);
        }
        session.take(command)
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

    /// Carries out a thread command the page sent for the session behind `token`, which writes
    /// the conversation of its round.
    fn thread_command(&self, token: &str, command: ThreadCommand) -> Result<(), CommandRefusal> {
        let mut sessions = self.lock();
        let session = find(&mut sessions, token).ok_or(CommandRefusal::Stale)?;
        if matches!(command, ThreadCommand::Retry { .. }) {
            session.actions.push("retry-messages".into());
        }
        session.threads.take(command)
    }

    /// The agent replies to the reviewer's latest message in the conversation of the latest
    /// round of the session behind `token`. Returns false when no session has that token, or
    /// when that conversation has no message.
    pub(crate) fn agent_replies(&self, token: &str) -> bool {
        let mut sessions = self.lock();
        let Some(session) = find(&mut sessions, token) else {
            return false;
        };
        let round = session.round_id();
        let text = session.data.chat_reply();
        session.threads.agent_replies(&round, text)
    }

    /// The wakeup for the reviewer's waiting messages of the session behind `token` did not
    /// reach the agent. Returns false when no session has that token, or no message waits.
    pub(crate) fn messages_not_delivered(&self, token: &str) -> bool {
        find(&mut self.lock(), token)
            .is_some_and(|session| session.threads.not_delivered(NOT_DELIVERED))
    }

    /// The messages the reviewer sent from the page of the session behind `token`, or `None`
    /// when no session has that token.
    pub(crate) fn messages(&self, token: &str) -> Option<Vec<SentMessage>> {
        find(&mut self.lock(), token).map(|session| session.threads.sent())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Session>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Rounds for Sessions {
    fn find(&self, token: &str) -> Option<PageRound> {
        let (stages, threads) = {
            let mut sessions = self.lock();
            let session = find(&mut sessions, token)?;
            (session.round.subscribe(), session.threads.subscribe())
        };
        let sessions = self.clone();
        let owner = token.to_owned();
        let commands = CommandSender::new(move |command, reply| {
            reply.send(sessions.command(&owner, command));
        });
        let sessions = self.clone();
        let owner = token.to_owned();
        let sender = ThreadSender::new(move |command, reply| {
            reply.send(sessions.thread_command(&owner, command));
        });
        Some(
            PageRound::new(stages, commands)
                .with_conversation(PageConversation::new(threads, sender)),
        )
    }
}

fn find<'a>(sessions: &'a mut [Session], token: &str) -> Option<&'a mut Session> {
    sessions
        .iter_mut()
        .find(|session| session.token.matches(token))
}
