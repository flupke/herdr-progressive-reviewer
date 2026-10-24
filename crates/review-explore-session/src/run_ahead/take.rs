//! Taking the forks of the question that waits: one per choice the setting names, each told
//! the prompt the agent would get after that answer, under the identities reserved for it.

use std::collections::BTreeMap;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use agent_fork::ProcessStamp;
use herdr_client::protocol::{Agent, PaneId};
use review_explore::{Alternative, AnswerInput, Comparison, ExploreRound, Question};
use review_explore_round_settings::RunAhead;
use review_explore_runner::{RoundData, TurnInstructions, Unreviewed};
use review_run_ahead::{Failure, ForkPoint, ForkRecord, ForkStart, RoundForks};
use review_types::MarkAuthor;
use tempfile::TempDir;

use super::settle::Trigger;
use super::{Event, ReservedIds, RoundKey, RunAheadInput, Taken, TakenFork};
use crate::turn_files::TurnFiles;
use crate::unreviewed_diffs::UnreviewedDiffs;
use crate::{ExploreSession, Input};

/// The files the forks of one question read: the unreviewed diffs their turns name, written
/// from a copy of the review marks with the question's marks applied, and each fork's
/// instructions, by its session.
pub(super) struct ForkFiles {
    _marks: TempDir,
    diffs: UnreviewedDiffs,
    turns: BTreeMap<String, TurnFiles>,
}

impl ForkFiles {
    /// The unreviewed diffs the forks' turns name.
    pub(super) fn diffs(&self) -> &UnreviewedDiffs {
        &self.diffs
    }

    /// The unreviewed diffs and the instructions of the fork whose session is `session`, which
    /// outlive the copy of the review marks the diffs were written from.
    pub(super) fn into_fork(mut self, session: &str) -> (UnreviewedDiffs, Option<TurnFiles>) {
        let turn = self.turns.remove(session);
        (self.diffs, turn)
    }
}

/// Why no fork is taken for the question that waits.
struct Refusal {
    why: String,
    /// Whether run-ahead failed: a check or a step it needs could not be made, rather than
    /// the forks waiting for something or the question needing none.
    failed: bool,
}

impl Refusal {
    /// Run-ahead failed, for `why`.
    fn failed(why: impl std::fmt::Display) -> Self {
        Self {
            why: why.to_string(),
            failed: true,
        }
    }
}

impl From<String> for Refusal {
    fn from(why: String) -> Self {
        Self { why, failed: false }
    }
}

impl From<&str> for Refusal {
    fn from(why: &str) -> Self {
        why.to_owned().into()
    }
}

/// The forks to start for the question that waits.
struct ForkPlan {
    point: ForkPoint,
    files: ForkFiles,
    forks: Vec<(TakenFork, String)>,
}

impl ExploreSession {
    /// Takes the forks of the question that waits, or logs why none can be taken.
    pub(super) fn run_ahead_take(&mut self) {
        match self.fork_plan() {
            Ok(plan) => self.start_forks(plan),
            Err(Refusal { why, failed }) => {
                let armed = self.run_ahead.armed.as_mut().expect("a question waits");
                if armed.refusal.as_ref() != Some(&why) {
                    armed.refusal = Some(why.clone());
                    self.run_ahead.log(&format!("no forks: {why}"));
                }
                if failed {
                    self.no_forks_failed(why);
                }
            }
        }
    }

    /// No fork could be taken for the question that waits, for `why`, a failure of run-ahead:
    /// it counts once per question, however often the refusal repeats.
    fn no_forks_failed(&mut self, why: String) {
        let Some(armed) = self.run_ahead.armed.as_mut() else {
            return;
        };
        if std::mem::replace(&mut armed.refusal_counted, true) {
            return;
        }
        let round = armed.asked.round.clone();
        self.run_ahead_failed(&round, Failure::NoForks { why });
    }

    /// The forks of the question that waits, prompts included, or why none can be taken.
    fn fork_plan(&mut self) -> Result<ForkPlan, Refusal> {
        let armed = self.run_ahead.armed.as_ref().ok_or("no question waits")?;
        if self.run_ahead.switching.is_some() {
            return Err("the agent is switching to a fork's session".into());
        }
        if self.run_ahead.settling.is_some() {
            return Err("the agent is going back to the session it ran before a fork".into());
        }
        let (round, pane) = (armed.asked.round.clone(), armed.asked.pane.clone());
        if let Some(refusal) = self
            .rounds
            .forks(&round.unit, &round.instance)
            .ok()
            .and_then(|forks| refusal(&forks, &armed.asked.question, self.run_ahead.reviewer))
        {
            return Err(refusal.into());
        }
        let question = armed.asked.question.clone();
        let agent = self.agent_to_fork(&pane)?;
        let choices = self.choices_to_prepare(&question)?;
        let point = self.run_ahead.host.point(&agent).map_err(Refusal::failed)?;
        let round = self.state.round.clone().ok_or("no round")?;
        let comparison = self.state.comparison.clone().ok_or("no change captured")?;
        let (unreviewed, mut files) = self
            .unreviewed_after_answer(&round)
            .map_err(|error| Refusal::failed(format!("{error:#}")))?;
        // A fork's turn carries the round's data as the agent's would.
        let round_data = self
            .round_data(
                &comparison.checkpoint.review_unit,
                &round.exploration.instance,
                false,
            )
            .map_err(|error| Refusal::failed(format!("{error:#}")))?;
        // Kept while the question waits: forks taken again are told the same.
        let armed = self.run_ahead.armed.as_mut().ok_or("no question waits")?;
        let forks = choices
            .iter()
            .map(|choice| {
                let reserved = armed
                    .reserved
                    .entry(choice.id.clone())
                    .or_insert_with(ReservedIds::new);
                fork_turn(
                    &round,
                    &question,
                    choice,
                    &comparison,
                    &unreviewed,
                    &round_data,
                    reserved,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(Refusal::failed)?
            .into_iter()
            .map(|(fork, turn)| {
                let prompt = turn.prompt().to_owned();
                files.turns.insert(fork.session.clone(), turn);
                (fork, prompt)
            })
            .collect();
        Ok(ForkPlan {
            point,
            files,
            forks,
        })
    }

    /// The agent of `pane`, once its session may be forked now, or why not.
    fn agent_to_fork(&mut self, pane: &PaneId) -> Result<Agent, Refusal> {
        let agent = self
            .agents
            .get_agent(pane)
            .map_err(Refusal::failed)?
            .ok_or("the agent's pane is gone")?;
        if !agent.agent_status.waits_for_prompt() {
            return Err(format!("the agent is {:?}", agent.agent_status).into());
        }
        // Forks taken from a session the round does not match would prepare turns of another
        // interview: the idle agent settles first, by itself once.
        if let Some((round, fork)) = self.unsettled() {
            let reason = format!(
                "the agent may run the session of the fork {}, whose turn the round did not take",
                fork.session
            );
            if !self.run_ahead.settle_failed.contains(&fork.session)
                && self.can_settle(&round, &fork, &agent)
            {
                self.settle(&round, fork, pane, reason.clone(), Trigger::RunAhead);
            }
            if self.unsettled().is_some() {
                return Err(reason.into());
            }
        }
        if let Some(why) = self.run_ahead.host.unforkable(&agent) {
            return Err(why.into());
        }
        // Run-ahead hears what the agent does through those hooks alone.
        if self.run_ahead.host.unhooked(&agent) {
            return Err(
                "the agent runs without the reviewer's Claude Code plugin: restart it".into(),
            );
        }
        Ok(agent)
    }

    /// The choices of `question` the reviewer's settings prepare.
    fn choices_to_prepare(&self, question: &Question) -> Result<Vec<Alternative>, Refusal> {
        let run_ahead = self.rounds.run_ahead().map_err(Refusal::failed)?;
        let choices: Vec<_> = question
            .alternatives
            .iter()
            .filter(|choice| run_ahead.prepares(choice.recommendation.is_some()))
            .cloned()
            .collect();
        if choices.is_empty() {
            return Err(match run_ahead {
                RunAhead::Off => "run-ahead is off",
                _ => "the question recommends no choice",
            }
            .into());
        }
        Ok(choices)
    }

    /// The unreviewed lines as the turn after an answer lists them: the current ones, less the
    /// lines the question's turn marks once the reviewer answers, worked out on a copy of the
    /// review marks.
    fn unreviewed_after_answer(
        &self,
        round: &ExploreRound,
    ) -> eyre::Result<(Unreviewed, ForkFiles)> {
        let snapshot = self.complete_snapshot()?;
        let marks = tempfile::Builder::new()
            .prefix("herdr-review-run-ahead-")
            .permissions(Permissions::from_mode(0o700))
            .tempdir()?;
        let tracker = self
            .tracker
            .copy_into(snapshot.identity.review_unit(), marks.path())?;
        if round.is_at(&snapshot.identity)
            && let Some(update) = round.exploration.waiting_turn()
        {
            // Who marks does not change which lines stay unreviewed.
            let author = MarkAuthor::Explore {
                answer: "run-ahead".into(),
            };
            crate::marks::mark_copy(&tracker, &snapshot, update, &author);
        }
        let (unreviewed, diffs) = self.write_unreviewed(&tracker, &snapshot)?;
        Ok((
            unreviewed,
            ForkFiles {
                _marks: marks,
                diffs,
                turns: BTreeMap::new(),
            },
        ))
    }

    /// Saves the record of the forks of `plan`, then starts them.
    fn start_forks(&mut self, plan: ForkPlan) {
        let round = self
            .run_ahead
            .armed
            .as_ref()
            .expect("a question waits")
            .asked
            .round
            .clone();
        let Some(taken_at_ms) = self.save_records(&round, &plan) else {
            return;
        };
        let mut forks = Vec::new();
        let mut listing = Vec::new();
        let mut halt = None;
        for (mut fork, prompt) in plan.forks {
            let start = ForkStart {
                point: &plan.point,
                session: &fork.session,
                prompt,
            };
            let started = self
                .run_ahead
                .host
                .start(start, self.ended_report(&round, &fork));
            let now = review_explore::now_ms();
            let failed = self.update_forks(&round, |saved| {
                let record = saved.fork_mut(&fork.session)?;
                match &started {
                    Ok(process) => record.process = Some(*process),
                    Err(error) => {
                        let exit = format!("did not start: {error}");
                        record.exit = Some(exit.clone());
                        record.cleaned = true;
                        return saved
                            .failed(Failure::Fork { exit }, now)
                            .then(|| saved.halt_notice())
                            .flatten();
                    }
                }
                None
            });
            if let Ok(Some(notice)) = failed {
                halt = Some(notice);
            }
            match started {
                Ok(process) => {
                    listing.push(format!("{} as {}", fork.choice, fork.session));
                    fork.process = Some(process);
                    forks.push(fork);
                }
                Err(error) => listing.push(format!("{} did not start: {error}", fork.choice)),
            }
        }
        self.run_ahead.log(&format!(
            "{} forks taken from session {} at entry {}, model {}: {}",
            forks.len(),
            plan.point.session,
            plan.point.entry.as_deref().unwrap_or("none"),
            plan.point.model.as_deref().unwrap_or("the agent's own"),
            listing.join(", "),
        ));
        let armed = self.run_ahead.armed.as_mut().expect("a question waits");
        armed.refusal = None;
        armed.taken = Some(Taken {
            point: plan.point,
            at_ms: taken_at_ms,
            forks,
            files: plan.files,
        });
        if let Some(notice) = halt {
            self.halt(&round, notice);
        }
    }

    /// Saves the records of the forks of `plan`, for the question that waits in `round`, before
    /// they start, so that a reviewer that dies meanwhile leaves their sessions named for the next
    /// one to delete. Checked under the lock of the record, so that two reviewers do not both
    /// fork the question. Returns when the forks were taken, once they may start.
    fn save_records(&mut self, round: &RoundKey, plan: &ForkPlan) -> Option<u64> {
        let armed = self.run_ahead.armed.as_ref().expect("a question waits");
        let (question, reviewer) = (armed.asked.question.clone(), self.run_ahead.reviewer);
        let taken_at_ms = review_explore::now_ms();
        let records: Vec<_> = plan
            .forks
            .iter()
            .map(|(fork, _)| ForkRecord {
                question: question.id.clone(),
                version: question.version,
                choice: fork.choice.clone(),
                answer: fork.request.answer.as_ref().map(|answer| answer.id.clone()),
                session: fork.session.clone(),
                from: Some(plan.point.session.clone()),
                transcripts: plan.point.transcripts.clone(),
                reviewer,
                process: None,
                taken_at_ms,
                turn: None,
                submitted_at_ms: None,
                exit: None,
                usage: None,
                discarded: None,
                cleaned: false,
                continued: None,
            })
            .collect();
        let saved = self.update_forks(round, |forks| {
            if let Some(refusal) = refusal(forks, &question, reviewer) {
                return Err(refusal);
            }
            forks.forks.extend(records);
            Ok(())
        });
        match saved {
            Ok(Ok(())) => Some(taken_at_ms),
            Ok(Err(refusal)) => {
                self.run_ahead.log(&format!("no forks: {refusal}"));
                if let Some(armed) = self.run_ahead.armed.as_mut() {
                    armed.refusal = Some(refusal);
                }
                None
            }
            Err(error) => {
                let why = format!("their record was not saved: {error}");
                self.run_ahead.log(&format!("no forks: {why}"));
                self.no_forks_failed(why);
                None
            }
        }
    }

    /// Where the fork `fork` of `round` reports its end: the session's inbox.
    fn ended_report(
        &self,
        round: &RoundKey,
        fork: &TakenFork,
    ) -> Box<dyn FnOnce(review_run_ahead::ForkEnd) + Send> {
        let inbox = self.inbox.clone();
        let (round, session) = (round.clone(), fork.session.clone());
        Box::new(move |end| {
            inbox.deliver(Input::RunAhead(RunAheadInput(Event::Ended {
                round,
                session,
                end,
            })));
        })
    }
}

/// Why no fork may be taken for `question`, as the forks of its round stand: run-ahead stopped
/// for the round, or forks of another reviewer that still runs answer it. This `reviewer`
/// takes the forks.
fn refusal(forks: &RoundForks, question: &Question, reviewer: ProcessStamp) -> Option<String> {
    if let Some(notice) = forks.halt_notice() {
        return Some(notice);
    }
    forks
        .forks
        .iter()
        .any(|fork| {
            question.is_version(&fork.question, fork.version)
                && fork.runs_for_another_reviewer(reviewer)
        })
        .then(|| "another reviewer forks this question".to_owned())
}

/// The fork that answers `question` with `choice`, the files of its turn and the prompt that
/// names them: the turn the agent would get after that answer, under the identities `reserved`
/// for the choice, with the fork's own access value.
fn fork_turn(
    round: &ExploreRound,
    question: &Question,
    choice: &Alternative,
    comparison: &Arc<Comparison>,
    unreviewed: &Unreviewed,
    round_data: &RoundData,
    reserved: &ReservedIds,
) -> Result<(TakenFork, TurnFiles), String> {
    let mut exploration = round.exploration.clone();
    let mut request = exploration
        .request(
            Some(AnswerInput {
                option: Some(choice.id.clone()),
                text: String::new(),
                in_reply_to: None,
                first_pick: None,
                drawing_board: false,
            }),
            Some(question),
        )
        .map_err(|error| format!("no turn for the choice {}: {error}", choice.id))?;
    reserved.apply(&mut request);
    let access = uuid::Uuid::new_v4().to_string();
    let answered = request
        .answer
        .as_ref()
        .and_then(|answer| round.exploration.answered_number(answer))
        .map(Into::into);
    let turn = TurnInstructions::prepare(
        &request, comparison, &access, unreviewed, round_data, answered,
    );
    let files = TurnFiles::write(&turn).map_err(|error| {
        format!(
            "the files of the turn for {} not written: {error}",
            choice.id
        )
    })?;
    let fork = TakenFork {
        session: uuid::Uuid::new_v4().to_string(),
        access,
        choice: choice.id.clone(),
        request,
        turn,
        process: None,
        kept: None,
        ended: false,
    };
    Ok((fork, files))
}
