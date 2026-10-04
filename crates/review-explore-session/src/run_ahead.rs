//! Run-ahead in the Explore session.
//!
//! While a question waits and the reviewer's settings turn run-ahead on, the session watches
//! the agent that asked it. Once that agent is idle, the session forks its session once per
//! choice the setting names. Each fork gets the prompt the agent would get after that answer,
//! with a request, an answer and an access value of its own, and takes a normal turn. The
//! session checks what a fork submits as it checks the agent's turns, keeps it for the fork's
//! choice beside the round, and shows it to nobody. When that agent works on something else
//! while the question waits and its session moves, the forks are taken again.
//!
//! When the reviewer's answer is exactly the one a fork was told, and the fork submitted its
//! turn, the pane's agent continues as that fork: it resumes the fork's session, then the
//! fork's turn becomes the round's (`answer.rs`, `switch.rs`). Every other answer goes to the
//! agent in the pane, as without run-ahead, and the session records why. Every fork the pane's
//! agent does not continue as is stopped and its transcript deleted once its question no
//! longer waits and when the reviewer closes; a reopened reviewer does the same for the forks a
//! stopped one left.

mod answer;
mod calls;
mod clean;
mod switch;
mod take;

use std::collections::HashSet;
use std::sync::Arc;

use agent_fork::ProcessStamp;
use herdr_client::protocol::{Agent, AgentStatus, PaneId};
use review_explore::{InterviewUpdate, Question, TurnRequest};
use review_run_ahead::{DiscardReason, ForkEnd, ForkHost, ForkPoint, PaneWatch, SwitchFailure};
use review_types::ReviewUnit;

use crate::{ExploreSession, Input};

/// An event of run-ahead's own threads, in the session's input order.
#[derive(Debug)]
pub struct RunAheadInput(Event);

#[derive(Debug)]
enum Event {
    /// Herdr reported `status` for the agent the question `generation` watches.
    Status {
        generation: u64,
        status: AgentStatus,
    },
    /// The fork `session` of the round `round` ended.
    Ended {
        round: RoundKey,
        session: String,
        end: ForkEnd,
    },
    /// The fork `session` of the round `round` is stopped and its transcript deleted.
    Cleaned { round: RoundKey, session: String },
    /// The switch of the pane's agent for the turn `request` of the round `round` ended: the
    /// agent as Herdr reports it on the fork's session, or why it failed.
    Switched {
        round: RoundKey,
        request: String,
        result: Box<Result<Agent, SwitchFailure>>,
    },
}

/// The round a fork belongs to: its review and its instance.
#[derive(Clone, Debug, Eq, PartialEq)]
struct RoundKey {
    unit: ReviewUnit,
    instance: String,
}

/// What the session keeps of run-ahead between inputs.
pub(crate) struct RunAheadState {
    host: Arc<dyn ForkHost>,
    /// This reviewer, as the forks it starts record it.
    reviewer: ProcessStamp,
    /// Counts the questions watched, so that a status from an earlier watch is ignored.
    generation: u64,
    /// The question that waits, and its forks.
    armed: Option<Armed>,
    /// The access values of the forks discarded: their calls are refused.
    discarded: HashSet<String>,
    /// The switch of the pane's agent to the fork an answer chose, while it runs.
    switching: Option<switch::Switching>,
    /// A turn saved while the switch runs, and its attempt: its prompt goes out once the
    /// switch ends.
    held: Option<crate::turn::SavedTurn>,
    /// The turns of the session's round, by request, that the pane's agent took as a fork.
    prepared: HashSet<String>,
}

/// A question that waits, and the agent watched for its forks.
struct Armed {
    generation: u64,
    asked: Asked,
    /// When the agent's turn asked the question, in milliseconds since the epoch: when the
    /// round knows it, else when the session began to watch the question.
    asked_at_ms: u64,
    _watch: PaneWatch,
    /// The forks taken, once the agent was idle.
    taken: Option<Taken>,
    /// Whether the agent worked since the forks were taken.
    worked: bool,
    /// Why no fork could be taken, as last logged.
    refusal: Option<String>,
}

/// A question run-ahead watches, in its round, and the agent that asked it.
#[derive(Clone)]
struct Asked {
    round: RoundKey,
    question: Question,
    /// Its number on the reviewer's screens, for the log.
    number: Option<usize>,
    /// The pane of the agent that asked it.
    pane: PaneId,
}

impl Asked {
    /// How the log names the question.
    fn label(&self) -> String {
        label(self.number)
    }
}

/// How the log names the question numbered `number` on the reviewer's screens.
fn label(number: Option<usize>) -> String {
    number.map_or_else(|| "Q?".to_owned(), |number| format!("Q{number}"))
}

/// The forks of one question, taken from one point of the agent's session.
struct Taken {
    point: ForkPoint,
    forks: Vec<TakenFork>,
    /// The files the forks' prompts name, which live as long as the forks.
    files: take::ForkFiles,
}

/// One fork taken for the question that waits: it runs, or ran.
#[derive(Clone)]
struct TakenFork {
    session: String,
    access: String,
    choice: String,
    /// The turn the fork was told, as if the reviewer had picked its choice, and its prompt.
    request: TurnRequest,
    prompt: String,
    process: Option<ProcessStamp>,
    /// The turn the fork submitted, kept for its choice.
    kept: Option<InterviewUpdate>,
    /// Whether its process ended.
    ended: bool,
}

impl RunAheadState {
    pub(crate) fn new(host: Arc<dyn ForkHost>) -> Self {
        Self {
            host,
            reviewer: ProcessStamp::read(std::process::id()),
            generation: 0,
            armed: None,
            discarded: HashSet::new(),
            switching: None,
            held: None,
            prepared: HashSet::new(),
        }
    }

    /// The turns of the session's round that the pane's agent took as a fork are now
    /// `prepared`, as a restored round records them.
    pub(crate) fn prepared_again(&mut self, prepared: impl Iterator<Item = String>) {
        self.prepared = prepared.collect();
    }

    /// The turns of the session's round, by request, that the pane's agent took as a fork.
    pub(crate) fn prepared_turns(&self) -> &HashSet<String> {
        &self.prepared
    }

    /// The fork that holds `access`, among those of the question that waits and the one the
    /// pane's agent is switching to.
    fn fork_with_access(&self, access: &str) -> Option<&TakenFork> {
        let waiting = self
            .armed
            .as_ref()
            .and_then(|armed| armed.taken.as_ref())
            .into_iter()
            .flat_map(|taken| &taken.forks);
        waiting
            .chain(self.switching.as_ref().map(|switching| &switching.fork))
            .find(|fork| fork.access == access)
    }

    /// The fork `session`, among those of the question that waits.
    fn fork_mut(&mut self, session: &str) -> Option<&mut TakenFork> {
        self.armed
            .as_mut()?
            .taken
            .as_mut()?
            .forks
            .iter_mut()
            .find(|fork| fork.session == session)
    }

    /// Logs `line` about the question that waits.
    fn log(&self, line: &str) {
        let label = label(self.armed.as_ref().and_then(|armed| armed.asked.number));
        self.host.log(&format!("{label}: {line}"));
    }
}

impl TakenFork {
    /// What stops the fork and removes what it left, its transcript among those of `point`.
    fn trace<'a>(&'a self, point: &'a ForkPoint) -> review_run_ahead::ForkTrace<'a> {
        review_run_ahead::ForkTrace {
            session: &self.session,
            process: self.process,
            transcripts: &point.transcripts,
        }
    }
}

impl Armed {
    fn is_for(&self, round: &RoundKey, question: &Question) -> bool {
        self.asked.round == *round
            && self
                .asked
                .question
                .is_version(&question.id, question.version)
    }
}

impl ExploreSession {
    /// Watches the question that waits for forks, and drops the forks of a question that no
    /// longer waits. Runs after every input.
    pub(crate) fn run_ahead_reconcile(&mut self) {
        let waiting = self.question_for_forks();
        let rounds = self.rounds.clone();
        let on = move || {
            rounds
                .run_ahead()
                .is_ok_and(review_explore_round_settings::RunAhead::is_on)
        };
        let armed_for_it = self.run_ahead.armed.as_ref().is_some_and(|armed| {
            waiting
                .as_ref()
                .is_some_and(|(round, question)| armed.is_for(round, question))
        });
        if armed_for_it {
            if !on() {
                self.run_ahead_discard(DiscardReason::TurnedOff);
            }
            return;
        }
        self.run_ahead_discard(DiscardReason::QuestionGone);
        if let Some((round, question)) = waiting
            && on()
        {
            self.run_ahead_arm(round, question);
        }
    }

    /// The question of the session's round that waits for the reviewer, when the round may
    /// still change.
    fn question_for_forks(&self) -> Option<(RoundKey, Question)> {
        if self.state.storage_error.is_some() || self.state.historical {
            return None;
        }
        let round = self.state.round.as_ref()?;
        let question = round.exploration.waiting_turn()?.next.clone()?;
        let key = RoundKey {
            unit: round.exploration.comparison.checkpoint.review_unit.clone(),
            instance: round.exploration.instance.clone(),
        };
        Some((key, question))
    }

    /// Changes the record of the forks of `round` with `change`.
    fn update_forks<T>(
        &self,
        round: &RoundKey,
        change: impl FnOnce(&mut review_run_ahead::RoundForks) -> T,
    ) -> review_store::Result<T> {
        self.rounds
            .update_forks(&round.unit, &round.instance, change)
    }

    /// Watches the agent that asked `question`; takes its forks at once when it is idle.
    fn run_ahead_arm(&mut self, round: RoundKey, question: Question) {
        let Some(agent) = self.round_agent() else {
            return;
        };
        self.run_ahead.generation += 1;
        let generation = self.run_ahead.generation;
        let inbox = self.inbox.clone();
        let watch = self.run_ahead.host.watch(
            &agent.pane_id,
            Box::new(move |status| {
                inbox.deliver(Input::RunAhead(RunAheadInput(Event::Status {
                    generation,
                    status,
                })));
            }),
        );
        let saved = self.state.round.as_ref();
        let number = saved.and_then(|round| round.exploration.question_number(&question));
        let asked_at_ms = saved
            .and_then(|round| asked_at_ms(round, &question))
            .unwrap_or_else(review_explore::now_ms);
        self.run_ahead.armed = Some(Armed {
            generation,
            asked: Asked {
                round,
                question,
                number,
                pane: agent.pane_id.clone(),
            },
            asked_at_ms,
            _watch: watch,
            taken: None,
            worked: false,
            refusal: None,
        });
        self.run_ahead
            .log("the question waits; its forks start once the agent is idle");
        // The watch reports changes only: the agent may be idle already.
        if self
            .agents
            .get_agent(&agent.pane_id)
            .ok()
            .flatten()
            .is_some_and(|agent| agent.agent_status.waits_for_prompt())
        {
            self.run_ahead_take();
        }
    }

    /// The agent the round's turns go to: the one pinned, else the one selected.
    fn round_agent(&mut self) -> Option<Agent> {
        if let Some(agent) = self
            .state
            .agent
            .as_ref()
            .and_then(|agent| agent.current(&*self.agents).ok().flatten())
        {
            return Some(agent);
        }
        self.target.resolve(&*self.agents).ok().flatten()
    }

    /// Follows an event of run-ahead's threads.
    pub(crate) fn run_ahead_input(&mut self, input: RunAheadInput) {
        match input.0 {
            Event::Status { generation, status } => self.run_ahead_status(generation, status),
            Event::Ended {
                round,
                session,
                end,
            } => self.run_ahead_ended(&round, &session, &end),
            Event::Cleaned { round, session } => self.run_ahead_cleaned(&round, &session),
            Event::Switched {
                round,
                request,
                result,
            } => self.run_ahead_switched(&round, &request, *result),
        }
    }

    /// The watched agent's `status`: its first idle takes the forks; an idle after work that
    /// moved its session takes them again.
    fn run_ahead_status(&mut self, generation: u64, status: AgentStatus) {
        let Some(armed) = self
            .run_ahead
            .armed
            .as_mut()
            .filter(|armed| armed.generation == generation)
        else {
            return;
        };
        if matches!(status, AgentStatus::Working | AgentStatus::Blocked) {
            armed.worked |= armed.taken.is_some();
            return;
        }
        if !status.waits_for_prompt() {
            return;
        }
        let Some(taken) = &armed.taken else {
            self.run_ahead_take();
            return;
        };
        if !std::mem::take(&mut armed.worked) {
            return;
        }
        let moved = self.run_ahead.host.last_entry(&taken.point) != taken.point.entry
            || self
                .agents
                .get_agent(&armed.asked.pane)
                .ok()
                .flatten()
                .and_then(|agent| agent.agent_session)
                .is_none_or(|session| session.value != taken.point.session);
        if moved {
            self.run_ahead
                .log("the agent's session moved since the forks were taken: taking them again");
            self.discard_taken(DiscardReason::SessionMoved);
            self.run_ahead_take();
        }
    }
}

/// When the agent's turn that asked `question` was saved in `round`, in milliseconds since the
/// epoch, when the round knows when its prompt went out and how long the agent took.
fn asked_at_ms(round: &review_explore::ExploreRound, question: &Question) -> Option<u64> {
    let request = &round
        .exploration
        .conversation
        .iter()
        .rfind(|turn| turn.update.next.as_ref() == Some(question))?
        .update
        .request;
    let started = round.turns.get(request)?.started_at_ms?;
    let elapsed = round.exploration.agent_elapsed_ms.get(request)?;
    Some(started.saturating_add(*elapsed))
}
