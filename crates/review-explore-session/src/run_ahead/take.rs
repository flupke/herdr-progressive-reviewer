//! Taking the forks of the question that waits: one per choice the setting names, each told
//! the prompt the agent would get after that answer.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use review_explore::{Alternative, AnswerInput, Comparison, ExploreRound, Question};
use review_explore_round_settings::RunAhead;
use review_explore_runner::{EarlierDecisions, PreparedTurn, Unreviewed};
use review_run_ahead::{ForkPoint, ForkRecord, ForkStart};
use review_types::MarkAuthor;
use tempfile::TempDir;

use super::{Event, RoundKey, RunAheadInput, Taken, TakenFork, is_idle};
use crate::unreviewed_diffs::UnreviewedDiffs;
use crate::{ExploreSession, Input};

/// The files the forks of one question read: the unreviewed diffs their prompts name, written
/// from a copy of the review marks with the question's marks applied.
pub(super) struct ForkFiles {
    _marks: TempDir,
    _diffs: UnreviewedDiffs,
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
        let question = armed.question.clone();
        let agent = self
            .agents
            .get_agent(&armed.pane)
            .map_err(|error| error.to_string())?
            .ok_or("the agent's pane is gone")?;
        if !is_idle(agent.agent_status) {
            return Err(format!("the agent is {:?}", agent.agent_status));
        }
        let point = self.run_ahead.host.point(&agent)?;
        let choices = self.choices_to_prepare(&question)?;
        let round = self.state.round.clone().ok_or("no round")?;
        let comparison = self.state.comparison.clone().ok_or("no change captured")?;
        let (unreviewed, files) = self
            .unreviewed_after_answer(&round)
            .map_err(|error| format!("{error:#}"))?;
        let forks = choices
            .iter()
            .map(|choice| fork_turn(&round, &question, choice, &comparison, &unreviewed))
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
                _diffs: diffs,
            },
        ))
    }

    /// Saves the record of the forks of `plan`, then starts them.
    fn start_forks(&mut self, plan: ForkPlan) {
        let armed = self.run_ahead.armed.as_ref().expect("a question waits");
        let round = armed.round.clone();
        let taken_at_ms = review_explore::now_ms();
        let records: Vec<_> = plan
            .forks
            .iter()
            .map(|(fork, _)| ForkRecord {
                question: armed.question.id.clone(),
                version: armed.question.version,
                choice: fork.choice.clone(),
                session: fork.session.clone(),
                transcripts: plan.point.transcripts.clone(),
                reviewer: self.run_ahead.reviewer,
                process: None,
                taken_at_ms,
                turn: None,
                exit: None,
                usage: None,
                discarded: None,
                cleaned: false,
            })
            .collect();
        // Saved before the forks start, so a reviewer that dies meanwhile leaves their sessions
        // named for the next one to delete.
        if let Err(error) = self.update_forks(&round, |forks| forks.forks.extend(records)) {
            self.run_ahead
                .log(&format!("no forks: their record was not saved: {error}"));
            return;
        }
        let mut forks = Vec::new();
        let mut listing = Vec::new();
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
            let _ = self.update_forks(&round, |saved| {
                let record = saved.fork_mut(&fork.session)?;
                match &started {
                    Ok(process) => record.process = Some(*process),
                    Err(error) => {
                        record.exit = Some(format!("did not start: {error}"));
                        record.cleaned = true;
                    }
                }
                Some(())
            });
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
            forks,
            _files: plan.files,
        });
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

/// The fork that answers `question` with `choice`, and the prompt it gets: the one the agent
/// would get after that answer, with the fork's own request, answer and access value.
fn fork_turn(
    round: &ExploreRound,
    question: &Question,
    choice: &Alternative,
    comparison: &Arc<Comparison>,
    unreviewed: &Unreviewed,
) -> Result<(TakenFork, String), String> {
    let mut exploration = round.exploration.clone();
    let request = exploration
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
        process: None,
        kept: None,
    };
    Ok((fork, prompt))
}
