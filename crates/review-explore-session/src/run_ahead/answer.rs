//! The reviewer's answer to a question run-ahead watched: the pane's agent continues as the
//! fork of the answer's choice when that fork's turn is exactly the one the agent would take
//! for the answer, and the agent can switch to it; otherwise the answer goes to the agent, as
//! without run-ahead. Either way the other forks are discarded, and the path is recorded with
//! its reason.

use review_explore::TurnRequest;
use review_run_ahead::{AnswerRecord, DiscardReason, PlainReason, TurnPath};

use super::switch::Switching;
use super::{Armed, Asked};
use crate::ExploreSession;
use crate::turn::SavedTurn;

/// Where the unreviewed diffs of a prompt stand, so that two prompts compare but for where
/// each one's diffs were written.
const DIFFS: &str = "<diffs>";

impl ExploreSession {
    /// `request` under the identities reserved for its choice, when it answers the question that
    /// waits with a choice whose forks were taken: with a comment too, so that the agent knows
    /// the answer by the same ID whichever path its turn takes.
    pub(crate) fn with_reserved_ids(&self, mut request: TurnRequest) -> TurnRequest {
        if let Some(reserved) = self
            .run_ahead
            .armed
            .as_ref()
            .and_then(|armed| armed.reserved_for(&request))
        {
            reserved.apply(&mut request);
        }
        request
    }

    /// Decides what the reviewer's answer, the saved turn `turn` with its marks applied, does
    /// with the forks of its question. Returns whether the pane's agent continues as a fork:
    /// its switch then runs, and the answer needs no prompt.
    pub(crate) fn run_ahead_answer(&mut self, turn: &SavedTurn) -> bool {
        let request = &turn.request;
        let Some(answer) = &request.answer else {
            return false;
        };
        let Some(armed) = &self.run_ahead.armed else {
            return false;
        };
        if !answer.question.as_ref().is_some_and(|question| {
            armed
                .asked
                .question
                .is_version(&question.id, question.version)
        }) {
            self.run_ahead_discard(DiscardReason::QuestionGone);
            return false;
        }
        let asked = armed.asked.clone();
        let chosen = self.fork_for(armed, request);
        let taken = self
            .run_ahead
            .armed
            .as_mut()
            .and_then(|armed| armed.taken.take());
        let path = match (chosen, taken) {
            (Ok(index), Some(mut taken)) => {
                let fork = taken.forks.remove(index);
                self.discard_forks(
                    &asked.round,
                    &taken.point,
                    &taken.forks,
                    DiscardReason::Answered,
                );
                self.continue_as(Switching {
                    asked: asked.clone(),
                    turn: turn.clone(),
                    dispatch: self.dispatch_of(request, &turn.attempt, None),
                    point: taken.point,
                    fork,
                    files: taken.files,
                    hold: self.prompts.hold(),
                })
            }
            (chosen, taken) => {
                if let Some(taken) = &taken {
                    self.discard_forks(
                        &asked.round,
                        &taken.point,
                        &taken.forks,
                        DiscardReason::Answered,
                    );
                }
                TurnPath::Plain {
                    reason: chosen.err().unwrap_or(PlainReason::NoForks { why: None }),
                }
            }
        };
        let continues = self.record_answer(&asked, request, &path);
        // The question no longer waits.
        self.run_ahead.armed = None;
        continues
    }

    /// Starts `switching`, when its fork's turn is exactly the agent's for the answer;
    /// otherwise discards the fork. Returns the path the answer's turn takes.
    fn continue_as(&mut self, switching: Switching) -> TurnPath {
        if let Err(reason) = self.same_turn(&switching) {
            self.discard_switching_fork(&switching);
            return TurnPath::Plain { reason };
        }
        let session = switching.fork.session.clone();
        match self.start_switch(switching) {
            Ok(()) => TurnPath::Prepared { session },
            Err(reason) => TurnPath::Plain { reason },
        }
    }

    /// The fork of `armed`, by index, that the answer `request` continues as, or why the answer
    /// goes to the pane's agent.
    fn fork_for(&self, armed: &Armed, request: &TurnRequest) -> Result<usize, PlainReason> {
        let answer = request.answer.as_ref().expect("an answer");
        // First, whatever the answer: run-ahead stays off for that agent until it restarts.
        if self
            .agents
            .get_agent(&armed.asked.pane)
            .ok()
            .flatten()
            .is_some_and(|agent| self.run_ahead.host.unhooked(&agent))
        {
            return Err(PlainReason::NoHooks);
        }
        let choice = answer
            .option
            .as_ref()
            .filter(|choice| !choice.is_none_of_the_above())
            .ok_or(PlainReason::NoneOfTheAbove)?;
        if !answer.text.trim().is_empty() {
            return Err(PlainReason::Comment);
        }
        if armed.talk.is_some() {
            return Err(PlainReason::ChatMessage);
        }
        let taken = armed.taken.as_ref().ok_or_else(|| PlainReason::NoForks {
            why: armed.refusal.clone(),
        })?;
        let index = taken
            .forks
            .iter()
            .position(|fork| fork.choice == choice.id)
            .ok_or(PlainReason::NotForked)?;
        let fork = &taken.forks[index];
        if fork.kept.is_none() {
            return Err(if fork.ended {
                PlainReason::NoTurn
            } else {
                PlainReason::StillWorking
            });
        }
        let unchecked = |error: &dyn std::fmt::Display| PlainReason::Unchecked {
            error: error.to_string(),
        };
        // A talk in the chat that the agent answered before the forks were taken is in their
        // sessions: forks taken again after it hold it.
        if self
            .rounds
            .reviewer_wrote_unknown_to_forks(
                &armed.asked.round.unit,
                &armed.asked.round.instance,
                &armed.asked.question.id,
                armed.asked_at_ms,
                taken.at_ms,
            )
            .map_err(|error| unchecked(&error))?
        {
            return Err(PlainReason::ChatMessage);
        }
        let agent = self
            .agents
            .get_agent(&armed.asked.pane)
            .map_err(|error| unchecked(&error))?
            .ok_or_else(|| unchecked(&"the agent's pane is gone"))?;
        if !agent.agent_status.waits_for_prompt() {
            return Err(PlainReason::AgentBusy);
        }
        let session = agent.agent_session.as_ref().map(|session| &session.value);
        if session != Some(&taken.point.session)
            || self.run_ahead.host.last_entry(&taken.point) != taken.point.entry
        {
            return Err(PlainReason::SessionMoved);
        }
        Ok(index)
    }

    /// Whether the agent's prompt for the answer of `switching` would be the prompt of its
    /// fork, but for where their diffs were written, and list the same unreviewed lines as the
    /// fork's diffs; otherwise why not. The session keeps the diffs of that prompt, as for a
    /// prompt it sends.
    fn same_turn(&mut self, switching: &Switching) -> Result<(), PlainReason> {
        let Switching {
            turn: SavedTurn { request, .. },
            fork,
            files,
            ..
        } = switching;
        let unchecked = |error: eyre::Report| PlainReason::Unchecked {
            error: format!("{error:#}"),
        };
        let comparison = self.turn_comparison(request).map_err(unchecked)?;
        let (prompt, unreviewed) = self
            .turn_prompt(request, &comparison, &fork.access)
            .map_err(unchecked)?;
        if !self
            .diffs
            .as_ref()
            .is_some_and(|diffs| diffs.same_lines(files.diffs()))
        {
            return Err(PlainReason::UnreviewedChanged);
        }
        let real = prompt.replace(&*unreviewed.directory.to_string_lossy(), DIFFS);
        let prepared = fork
            .prompt
            .replace(&*files.diffs().directory().to_string_lossy(), DIFFS);
        if real != prepared {
            return Err(PlainReason::PromptChanged);
        }
        Ok(())
    }

    /// Records the path `path` of the answer `request` to the question `asked`, and logs it.
    /// The reviewer is shown the path of a plain chain at once: it shows with the turn once the
    /// agent took it. Returns whether the pane's agent continues as a fork.
    fn record_answer(&mut self, asked: &Asked, request: &TurnRequest, path: &TurnPath) -> bool {
        let answer = request.answer.as_ref().expect("an answer");
        let record = AnswerRecord {
            question: asked.question.id.clone(),
            version: asked.question.version,
            answer: answer.id.clone(),
            request: request.request.clone(),
            at_ms: review_explore::now_ms(),
            path: path.clone(),
        };
        let line = match path {
            TurnPath::Prepared { session } => {
                format!("the answer continues as the fork {session}: switching")
            }
            TurnPath::Plain { reason } => {
                format!("the answer goes to the agent in the pane: {reason:?}")
            }
        };
        self.run_ahead.log(&line);
        if let Err(error) = self.update_forks(&asked.round, |forks| forks.answers.push(record)) {
            self.run_ahead
                .log(&format!("the answer's path was not recorded: {error}"));
        }
        // A prepared turn shows once the pane's agent runs its fork's session.
        if matches!(path, TurnPath::Prepared { .. }) {
            return true;
        }
        self.show_path(&asked.round, &request.request, path.clone());
        false
    }
}
