//! Taking the forks of the question that waits: one per choice the setting names, each told
//! the prompt the agent would get after that answer, under the identities reserved for it.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use agent_fork::ProcessStamp;
use review_explore::{Alternative, AnswerInput, Comparison, ExploreRound, Question};
use review_explore_round_settings::RunAhead;
use review_explore_runner::{EarlierDecisions, PreparedTurn, Unreviewed};
use review_run_ahead::{FAILURES_TO_HALT, ForkPoint, ForkRecord, ForkStart, RoundForks};
use review_types::MarkAuthor;
use tempfile::TempDir;

use super::settle::Trigger;
use super::{Event, ReservedIds, RoundKey, RunAheadInput, Taken, TakenFork};
use crate::unreviewed_diffs::UnreviewedDiffs;
use crate::{ExploreSession, Input};

/// The files the forks of one question read: the unreviewed diffs their prompts name, written
/// from a copy of the review marks with the question's marks applied.
pub(super) struct ForkFiles {
    _marks: TempDir,
    diffs: UnreviewedDiffs,
}

impl ForkFiles {
    /// The unreviewed diffs the forks' prompts name.
    pub(super) fn diffs(&self) -> &UnreviewedDiffs {
        &self.diffs
    }

    /// The unreviewed diffs, which outlive the copy of the review marks they were written from.
    pub(super) fn into_diffs(self) -> UnreviewedDiffs {
        self.diffs
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
            Err(reason) => {
                let armed = self.run_ahead.armed.as_mut().expect("a question waits");
                if armed.refusal.as_ref() != Some(&reason) {
                    armed.refusal = Some(reason.clone());
                    self.run_ahead.log(&format!("no forks: {reason}"));
                }
            }
        }
    }

    /// The forks of the question that waits, prompts included, or why none can be taken.
    fn fork_plan(&mut self) -> Result<ForkPlan, String> {
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
            return Err(refusal);
        }
        let question = armed.asked.question.clone();
        let agent = self
            .agents
            .get_agent(&pane)
            .map_err(|error| error.to_string())?
            .ok_or("the agent's pane is gone")?;
        if !agent.agent_status.waits_for_prompt() {
            return Err(format!("the agent is {:?}", agent.agent_status));
        }
        // Forks taken from a session the round does not match would prepare turns of another
        // interview: the idle agent settles first, by itself once.
        if let Some((round, fork)) = self.unsettled() {
            let reason = format!(
                "the agent may run the session of the fork {}, whose turn the round did not take",
                fork.session
            );
            if !self.run_ahead.settle_failed.contains(&fork.session) {
                self.settle(&round, fork, &pane, reason.clone(), Trigger::RunAhead);
            }
            return Err(reason);
        }
        let point = self.run_ahead.host.point(&agent)?;
        let choices = self.choices_to_prepare(&question)?;
        let round = self.state.round.clone().ok_or("no round")?;
        let comparison = self.state.comparison.clone().ok_or("no change captured")?;
        let (unreviewed, files) = self
            .unreviewed_after_answer(&round)
            .map_err(|error| format!("{error:#}"))?;
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
                    reserved,
                )
            })
            .collect::<Result<_, _>>()?;
        Ok(ForkPlan {
            point,
            files,
            forks,
        })
    }

    /// The choices of `question` the reviewer's settings prepare.
    fn choices_to_prepare(&self, question: &Question) -> Result<Vec<Alternative>, String> {
        let run_ahead = self.rounds.run_ahead().map_err(|error| error.to_string())?;
        let choices: Vec<_> = question
            .alternatives
            .iter()
            .filter(|choice| run_ahead.prepares(choice.recommendation.is_some()))
            .cloned()
            .collect();
        if choices.is_empty() {
            return Err(match run_ahead {
                RunAhead::Off => "run-ahead is off".into(),
                _ => "the question recommends no choice".into(),
            });
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
        let mut halted = false;
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
                        record.exit = Some(format!("did not start: {error}"));
                        record.cleaned = true;
                        return Some(saved.fork_failed(now));
                    }
                }
                Some(false)
            });
            halted |= matches!(failed, Ok(Some(true)));
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
        if halted {
            self.halt(&round);
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
                self.run_ahead
                    .log(&format!("no forks: their record was not saved: {error}"));
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
    if forks.halted_at_ms.is_some() {
        return Some(halted());
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

/// Why no fork is taken once too many failed in a row.
pub(super) fn halted() -> String {
    format!("Run-ahead stopped for this round: {FAILURES_TO_HALT} forks in a row failed")
}

/// The fork that answers `question` with `choice`, and the prompt it gets: the one the agent
/// would get after that answer, under the identities `reserved` for the choice, with the
/// fork's own access value.
fn fork_turn(
    round: &ExploreRound,
    question: &Question,
    choice: &Alternative,
    comparison: &Arc<Comparison>,
    unreviewed: &Unreviewed,
    reserved: &ReservedIds,
) -> Result<(TakenFork, String), String> {
    let mut exploration = round.exploration.clone();
    let mut request = exploration
        .request(
            Some(AnswerInput {
                option: Some(choice.id.clone()),
                text: String::new(),
                in_reply_to: None,
                first_pick: None,
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
    let prompt = PreparedTurn::prepare(
        &request,
        comparison,
        &access,
        unreviewed,
        &EarlierDecisions::default(),
        answered,
    )
    .prompt();
    let fork = TakenFork {
        session: uuid::Uuid::new_v4().to_string(),
        access,
        choice: choice.id.clone(),
        request,
        prompt: prompt.clone(),
        process: None,
        kept: None,
        ended: false,
    };
    Ok((fork, prompt))
}
