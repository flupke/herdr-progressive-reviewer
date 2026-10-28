//! What the Explore page shows of the round the session owns, and the commands the reviewer
//! sends from it.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use review_explore::{
    CitedLines, CodeLocation, Comparison, DispatchState, EvidenceRef, Exploration, ExploreRound,
    ImplementationDelivery, KeptAnswer, Question, ReviewerAnswer, RoundOverview, TurnMarks,
    TurnRequest, Uncitable,
};
use review_explore_citations::{Citation, CodeColors};
use review_explore_page::{
    Answered, AnsweredQuestion, CommandRefusal, CommandReply, ImplementationState, Interruption,
    LatestAnswer, PageAnswer, PageCommand, PageImplement, PageImplementation, PageQuiz,
    PublishedRound, QuestionMarks, Recovery, ReviewName, RoundStage, SentAnswer, TurnResponse,
};
use review_repository::repository::{Repository, SnapshotIdentity};
use review_run_ahead::TurnPath;
use review_source::ReviewCheckpoint;

use crate::{ExploreSession, Start};

/// The citations the page shows, those of a question, the proofs of a conclusion's quiz or those
/// of the earlier questions, found in the change and colored once per stage rather than on
/// every input the session handles.
#[derive(Default)]
pub(crate) struct PageCitations {
    colors: CodeColors,
    shown: Option<Shown>,
}

/// The lists of citations the page shows, and the checkpoint they were found at.
struct Shown {
    checkpoint: ReviewCheckpoint,
    lists: Vec<Vec<EvidenceRef>>,
    citations: Vec<Arc<[Citation]>>,
}

/// Where the page reads the lines a citation names: the working copy, for the round the session
/// owns, or the repository at a saved round's checkpoint, for a past round.
#[derive(Clone, Copy)]
pub(crate) enum CitedCode<'a> {
    /// The working copy at this root.
    WorkingCopy(&'a Path),
    /// The repository, at the round's checkpoint, else the round's saved diff.
    Saved(&'a Repository),
}

impl CitedCode<'_> {
    /// The lines `location` names in the change `comparison`.
    fn lines(
        self,
        comparison: &Comparison,
        location: &CodeLocation,
    ) -> Result<CitedLines, Uncitable> {
        match self {
            // The page may be open from the network: it shows no file outside the change and
            // the repository's tracked files.
            Self::WorkingCopy(root) => comparison.tracked_cited_lines(location, root),
            Self::Saved(repository) => comparison.saved_cited_lines(location, repository),
        }
    }
}

impl PageCitations {
    /// The citations of `question`, with the lines `comparison` has for them in `code`.
    fn of(
        &mut self,
        question: &Question,
        comparison: &Comparison,
        code: CitedCode<'_>,
    ) -> Arc<[Citation]> {
        self.lists(std::slice::from_ref(&question.evidence), comparison, code)
            .swap_remove(0)
    }

    /// Each list of `lists` cited, with the lines `comparison` has for them in `code`.
    pub(crate) fn lists(
        &mut self,
        lists: &[Vec<EvidenceRef>],
        comparison: &Comparison,
        code: CitedCode<'_>,
    ) -> Vec<Arc<[Citation]>> {
        if let Some(shown) = &self.shown
            && shown.checkpoint == comparison.checkpoint
            && shown.lists == lists
        {
            return shown.citations.clone();
        }
        let citations: Vec<Arc<[Citation]>> = lists
            .iter()
            .map(|list| {
                list.iter()
                    .map(|evidence| {
                        let lines = code.lines(comparison, &evidence.location);
                        self.colors.cite(evidence.clone(), lines)
                    })
                    .collect()
            })
            .collect();
        self.shown = Some(Shown {
            checkpoint: comparison.checkpoint.clone(),
            lists: lists.to_vec(),
            citations: citations.clone(),
        });
        citations
    }
}

impl ExploreSession {
    /// Publishes the stage of the round the pane shows; the page loads itself again only when
    /// the stage changed.
    pub(crate) fn publish_page(&mut self) {
        let stage = self.page_stage();
        let cancellable = self.cancellable();
        let overview = self.state.round.as_ref().map(|round| {
            // The turn this process delivers to the agent now, as `page_stage` tells it.
            let delivering = self
                .state
                .pending
                .as_ref()
                .filter(|(instance, _)| *instance == round.exploration.instance)
                .map(|(_, request)| request.as_str());
            RoundOverview::of(round, delivering)
        });
        let earlier_citations = self
            .state
            .round
            .as_ref()
            .zip(overview.as_ref())
            .map_or_else(Vec::new, |(round, overview)| {
                let lists: Vec<Vec<EvidenceRef>> = overview
                    .earlier
                    .iter()
                    .map(|record| record.question.evidence.clone())
                    .collect();
                self.earlier_citations.lists(
                    &lists,
                    &round.exploration.comparison,
                    CitedCode::WorkingCopy(self.repository.root()),
                )
            });
        let round = self
            .state
            .round
            .as_ref()
            .zip(overview.as_ref())
            .map(|(round, overview)| PublishedRound {
                id: &round.exploration.instance,
                review_unit: &round.exploration.comparison.checkpoint.review_unit,
                design: round.exploration.design(),
                changed_files: round.exploration.comparison.files.len(),
                author: round.exploration.author,
                cancellable: cancellable.as_ref(),
                earlier: self.state.historical,
                overview,
                earlier_citations: &earlier_citations,
            });
        self.page.publish_counted(round, stage, self.mark_tally());
    }

    /// The reviewer's latest answer, while the reviewer may cancel it, as the pane offers it:
    /// in a round that can still change, before any implementation request.
    pub(crate) fn cancellable(&self) -> Option<LatestAnswer> {
        let round = self.state.round.as_ref()?;
        if self.state.historical
            || self.state.storage_error.is_some()
            || !round.implementations.is_empty()
        {
            return None;
        }
        let answer = round.exploration.answers.last()?;
        Some(LatestAnswer {
            id: answer.id.clone(),
            choice: KeptAnswer::of(answer).choice,
            comment: answer.text.clone(),
            answered: answered(answer),
        })
    }

    /// The reviewer now shows the snapshot `identity`: the page names the review it belongs to,
    /// as the pane's header does.
    pub fn name_review(&self, identity: &SnapshotIdentity) {
        self.page
            .name(ReviewName::of(self.repository.root(), identity));
    }

    /// Carries out `command` from the page, or refuses it, and replies.
    pub(crate) fn page_command(&mut self, command: PageCommand, reply: CommandReply) {
        match command {
            PageCommand::Answer(answer) => {
                let result = self.answer_from_page(answer);
                // The new stage reaches the page before the reply does.
                self.publish_page();
                reply.send(result);
            }
            PageCommand::Pick { question, version } => {
                let result = self.pick_from_page(&question, version);
                reply.send(result);
            }
            PageCommand::DiagramFailed(error) => {
                let result = self.diagram_failed(error);
                self.publish_page();
                reply.send(result);
            }
            // A reviewer's worker calls `start_from_page` itself, so that Jev marks before the
            // kickoff: this sends the kickoff at once.
            PageCommand::Start { choices, start } => {
                if let Some(kickoff) = self.start_from_page(choices, &start, reply) {
                    let _ = self.deliver_turn(kickoff, None);
                }
            }
            PageCommand::Implement(implement) => {
                let result = self.implement_from_page(implement);
                self.publish_page();
                reply.send(result);
            }
            PageCommand::Quiz(response) => {
                let result = self.quiz_from_page(response);
                self.publish_page();
                reply.send(result);
            }
            PageCommand::Recover(recovery) => {
                self.recover_from_page(recovery, reply);
            }
        }
    }

    /// Carries out the page's action that recovers or closes the round, or refuses it, and
    /// replies, then returns whether it went through. A reviewer's worker drops the kickoff it
    /// holds once a Stop waiting or a Reset went through.
    pub fn recover_from_page(&mut self, recovery: Recovery, reply: CommandReply) -> bool {
        let result = match recovery {
            Recovery::Stop(waiting) => self.stop(&waiting),
            Recovery::Reset { round } => self.reset_round(&round),
            Recovery::Retry { request, attempt } => self.retry_from_page(&request, &attempt),
            Recovery::CancelAnswer { answer } => self.cancel_answer_from_page(answer),
            Recovery::CancelImplementation { delivery } => {
                self.cancel_implementation_from_page(&delivery)
            }
            Recovery::ResendImplementation {
                conclusion,
                delivery,
                attempt,
            } => self.resend_from_page(&conclusion, &delivery, &attempt),
        };
        self.reply_to_page(reply, result)
    }

    /// Replies `result` to the page once it shows the new stage: the page loads itself again
    /// once it has the reply. Returns whether the command went through.
    fn reply_to_page(&mut self, reply: CommandReply, result: Result<(), CommandRefusal>) -> bool {
        self.publish_page();
        let done = result.is_ok();
        reply.send(result);
        done
    }

    /// The latest saved copy of the round the pane shows, so that the page never acts on a
    /// stale copy.
    pub(crate) fn saved_round(&self) -> Result<ExploreRound, CommandRefusal> {
        let shown = self.state.round.as_ref().ok_or(CommandRefusal::Stale)?;
        self.rounds
            .round(
                &shown.exploration.comparison.checkpoint.review_unit,
                &shown.exploration.instance,
            )
            .map_err(|error| CommandRefusal::Failed(error.to_string()))?
            .ok_or(CommandRefusal::Stale)
    }

    /// Answers the question the page showed, as the pane would answer it. The turn comes from
    /// the latest saved round, so the page never answers from a stale copy, and a question
    /// answered already, from the pane or from another page, is refused.
    fn answer_from_page(&mut self, answer: PageAnswer) -> Result<(), CommandRefusal> {
        let failed = |error: &dyn std::fmt::Display| CommandRefusal::Failed(error.to_string());
        let round = self.saved_round()?;
        let Some(question) = waiting_question(&round.exploration)
            .filter(|question| question.is_version(&answer.question, answer.version))
            .cloned()
        else {
            // The round's latest answer is this one: a repeat of an answer that went through.
            let repeat = round.exploration.answers.last().is_some_and(|latest| {
                let latest_answered = answered(latest);
                latest_answered.question == Some((answer.question.clone(), answer.version))
                    && latest_answered.option == answer.input.option
                    && latest.text == answer.input.text
            });
            return Err(CommandRefusal::stale_unless_repeat(repeat));
        };
        let request = round
            .exploration
            .clone()
            .request(Some(answer.input), Some(&question))
            .map_err(|error| failed(&error))?;
        self.deliver_turn(request, None)
            .map_err(CommandRefusal::Failed)
    }

    /// Accepts the reviewer's first pick of version `version` of `question`, which the page
    /// keeps, while the latest saved round still waits for an answer to it. A pick saves
    /// nothing: the answer carries it.
    fn pick_from_page(&self, question: &str, version: u32) -> Result<(), CommandRefusal> {
        let round = self.saved_round()?;
        waiting_question(&round.exploration)
            .filter(|waiting| waiting.is_version(question, version))
            .map(|_| ())
            .ok_or(CommandRefusal::Stale)
    }

    /// Implements the conclusion the page showed, as the pane would: the request comes from
    /// the latest saved round, and an empty list is refused. The page may send one only while
    /// the conclusion has no request that is on its way or that the agent received, as the pane
    /// offers it, and only in place of the request it showed, so that a repeated or stale
    /// Implement cannot start a second implementation.
    fn implement_from_page(&mut self, implement: PageImplement) -> Result<(), CommandRefusal> {
        let round = self.saved_round()?;
        if round.exploration.conclusion_request() != Some(implement.conclusion.as_str()) {
            return Err(CommandRefusal::Stale);
        }
        let sending = self.state.implementation.is_some();
        let offered = match round.latest_implementation(&implement.conclusion) {
            None => implement.replaces.is_none(),
            Some(latest) => {
                page_implementation(latest, sending).state.allows_another()
                    && implement.replaces.as_deref() == Some(latest.request.delivery.as_str())
            }
        };
        if !offered {
            // The conclusion's latest request is this list, on its way or received, in place of
            // the one the page showed: a repeat of an Implement that went through.
            let repeat = round
                .latest_implementation(&implement.conclusion)
                .is_some_and(|latest| {
                    latest.request.text == implement.text
                        && !page_implementation(latest, sending).state.allows_another()
                        && implement.replaces.as_deref() != Some(latest.request.delivery.as_str())
                });
            return Err(CommandRefusal::stale_unless_repeat(repeat));
        }
        let request = round
            .exploration
            .implementation(implement.text)
            .map_err(|error| CommandRefusal::Failed(error.to_string()))?;
        self.implement(request).map_err(CommandRefusal::Failed)
    }

    fn page_stage(&mut self) -> RoundStage {
        if let Some(failure) = &self.state.storage_error {
            return RoundStage::StorageFailed {
                failure: failure.clone(),
            };
        }
        let Some(round) = &self.state.round else {
            return match &self.state.start {
                Start::Idle { offer } => RoundStage::NoRound {
                    start: offer.clone(),
                },
                Start::Starting {
                    start,
                    started_at_ms,
                } => RoundStage::Starting {
                    start: start.clone(),
                    started_at_ms: Some(*started_at_ms),
                },
                Start::Failed { failure, offer } => RoundStage::StartFailed {
                    failure: failure.clone(),
                    start: offer.clone(),
                },
            };
        };
        let exploration = &round.exploration;
        let code = CitedCode::WorkingCopy(self.repository.root());
        if let Some(request) = exploration.pending_request() {
            // A turn saved as pending that no prompt of this process carries, after a
            // reopening or Stop waiting, waits for Retry.
            let delivering = self.state.pending.as_ref().is_some_and(|(instance, id)| {
                *instance == request.instance && *id == request.request
            });
            let answer = sent_answer(round, request, &mut self.citations, code);
            return if delivering {
                RoundStage::AgentWorking {
                    request: request.request.clone(),
                    sent_at_ms: round
                        .turns
                        .get(&request.request)
                        .and_then(|delivery| delivery.started_at_ms),
                    finishing_since_ms: self.run_ahead_finishing_since(&request.request),
                    answer,
                }
            } else {
                interrupted(round, request, answer)
            };
        }
        if let Some(retry) = exploration.retry_request() {
            let answer = sent_answer(round, retry, &mut self.citations, code);
            return interrupted(round, retry, answer);
        }
        let sending = self.state.implementation.is_some();
        // What `save_from_page` refuses, the page does not ask.
        let takes_quiz_answers = !self.state.historical && self.state.storage_error.is_none();
        latest_turn(
            round,
            LatestTurn {
                sending,
                takes_quiz_answers,
                paths: self.run_ahead.turn_paths(),
            },
            &mut self.citations,
            code,
        )
    }
}

/// The reviewer's answer that the turn `request` of `round` carries, with the question it
/// answers, whose citations `citations` finds in the change in `code`, and the marks it applied.
pub(crate) fn sent_answer(
    round: &ExploreRound,
    request: &TurnRequest,
    citations: &mut PageCitations,
    code: CitedCode<'_>,
) -> Option<Box<SentAnswer>> {
    let answer = request.answer.as_ref()?;
    Some(Box::new(SentAnswer {
        question: answer.question.as_ref().map(|question| AnsweredQuestion {
            citations: citations.of(question, &round.exploration.comparison, code),
            question: Box::new(question.clone()),
            picked_blind: answer.first_pick.is_some(),
        }),
        kept: KeptAnswer::of(answer),
        // The marks of the turn that asked the question, which the answer applied.
        marked: round
            .marks
            .get(&answer.in_reply_to)
            .filter(|marks| marks.answer.as_deref() == Some(answer.id.as_str()))
            .map(TurnMarks::counts)
            .unwrap_or_default(),
    }))
}

/// The turn `request` of `round`, which the agent is not working on, and why; `answer` is the
/// reviewer's answer the turn carries.
pub(crate) fn interrupted(
    round: &ExploreRound,
    request: &TurnRequest,
    answer: Option<Box<SentAnswer>>,
) -> RoundStage {
    let delivery = round.turns.get(&request.request);
    let state = delivery.map(|delivery| &delivery.state);
    let interruption = match (state, &request.response_error) {
        (Some(DispatchState::NotStarted), _) => Interruption::NotStarted,
        (_, Some(failure)) => Interruption::Failed(failure.clone()),
        (Some(DispatchState::Attempting | DispatchState::Unknown), None) => Interruption::Uncertain,
        (_, None) => Interruption::Stopped,
    };
    RoundStage::Interrupted {
        request: Some(request.request.clone()),
        attempt: delivery.map(|delivery| delivery.attempt.clone()),
        interruption,
        answer,
    }
}

/// The question the round waits for an answer to: the one the agent's latest turn posted,
/// unless a turn is pending or waits for Retry.
fn waiting_question(exploration: &Exploration) -> Option<&Question> {
    exploration.waiting_turn()?.next.as_ref()
}

/// What the page shows of the agent's latest turn besides the turn itself.
#[derive(Clone, Copy)]
pub(crate) struct LatestTurn<'a> {
    /// Whether this process sends an implementation request.
    pub(crate) sending: bool,
    /// Whether the round can save the reviewer's answers to the conclusion's quiz.
    pub(crate) takes_quiz_answers: bool,
    /// The path each turn took after an answer to a question run-ahead watched, by request.
    pub(crate) paths: &'a BTreeMap<String, TurnPath>,
}

/// The question or conclusion the agent's latest turn posted, as `latest` says to show it, with
/// its citations' lines from `code`.
pub(crate) fn latest_turn(
    round: &ExploreRound,
    latest: LatestTurn<'_>,
    citations: &mut PageCitations,
    code: CitedCode<'_>,
) -> RoundStage {
    let exploration = &round.exploration;
    // A round with no turn to answer and none to send again: only Reset is left.
    let nothing = || RoundStage::Interrupted {
        request: None,
        attempt: None,
        interruption: Interruption::Stopped,
        answer: None,
    };
    let Some(turn) = exploration.conversation.last() else {
        return nothing();
    };
    let response = TurnResponse {
        path: latest.paths.get(&turn.update.request).cloned(),
        ..TurnResponse::of(exploration, turn)
    };
    if let Some(conclusion) = &turn.update.conclusion {
        let request = &turn.update.request;
        let proofs: Vec<Vec<EvidenceRef>> = conclusion
            .quiz
            .iter()
            .map(|item| item.proof.clone())
            .collect();
        return RoundStage::Conclusion {
            request: request.clone(),
            conclusion: Box::new(conclusion.clone()),
            implementation: round
                .latest_implementation(request)
                .map(|delivery| page_implementation(delivery, latest.sending)),
            quiz: PageQuiz {
                proofs: citations.lists(&proofs, &exploration.comparison, code),
                answers: exploration
                    .quiz_answers(request)
                    .cloned()
                    .unwrap_or_default(),
                takes_answers: latest.takes_quiz_answers,
            },
            response,
        };
    }
    match &turn.update.next {
        Some(question) => RoundStage::Question {
            number: exploration.questions.len(),
            question: Box::new(question.clone()),
            citations: citations.of(question, &exploration.comparison, code),
            marks: QuestionMarks::requested(&turn.update),
            response,
            answer_cancelled: exploration.cancelled_since_last_turn(),
        },
        None => nothing(),
    }
}

/// An implementation request as the page shows it.
fn page_implementation(delivery: &ImplementationDelivery, sending: bool) -> PageImplementation {
    let state = ImplementationState::of(&delivery.state, sending);
    PageImplementation {
        delivery: delivery.request.delivery.clone(),
        attempt: delivery.attempt.clone(),
        text: delivery.request.text.clone(),
        state,
        sent_at_ms: delivery.sent_at_ms,
    }
}

/// What `answer` answered, so that the page knows a repeat of it.
pub(crate) fn answered(answer: &ReviewerAnswer) -> Answered {
    Answered {
        question: answer
            .question
            .as_ref()
            .map(|question| (question.id.clone(), question.version)),
        option: answer.option.as_ref().map(|option| option.id.clone()),
        in_reply_to: answer.in_reply_to.clone(),
    }
}
