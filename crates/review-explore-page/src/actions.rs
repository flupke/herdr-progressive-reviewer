//! The reviewer's actions, as the page's socket receives them. Each is checked against the
//! latest stage the round's owner published, then handed to the owner as a [`PageCommand`],
//! which checks it again against its own round: an action on something the round moved past
//! is refused as stale, with a notice worded for it. A repeat of an action that went through
//! (the same answer to the same version of a question, the same pick, list, quiz answer or
//! reply) is answered as applied already, and changes nothing; a repeat of a Start, a Stop
//! waiting, a Retry or a resend names a start, a turn or an attempt that is no longer the
//! current one, so it cannot start, stop or send anything a second time. A message in the
//! round's conversation goes to the owner of the review threads instead (`conversation`).

mod conversation;

use review_explore::{AnswerInput, DiagramError, QuizResponse};

use crate::blind::Pick;
use crate::command::{PageAnswer, PageCommand, PageImplement, PageQuizResponse};
use crate::notice::{Action, Notice, Problem, RecoveryAction};
use crate::page::ExplorePage;
use crate::round::PageRound;
use crate::round::{Answered, ImplementationState, Interruption, RoundStage};
use crate::rpc::{
    AnswerParams, Call, ConclusionCall, ImplementParams, Outcome, PickParams, QuizParams,
    ResendImplementationParams, ResetParams, RetryParams, RoundCall, StartParams, StopParams,
};
use crate::{Recovery, Rounds, Waiting};

/// The actions of one page on one round.
pub(crate) struct Actions<'a, R> {
    pub(crate) page: &'a ExplorePage<R>,
    pub(crate) round: &'a PageRound,
}

impl<R: Rounds> Actions<'_, R> {
    /// Carries out `call`, or says why not.
    pub(crate) async fn call(&self, call: Call) -> Result<Outcome, Notice> {
        match call {
            Call::Round(call) => self.on_round(call).await,
            Call::Conclusion(call) => self.on_conclusion(call).await,
            Call::Conversation(call) => self.in_conversation(call).await,
        }
    }

    async fn on_round(&self, call: RoundCall) -> Result<Outcome, Notice> {
        match call {
            RoundCall::Answer(params) => self.answer(params).await,
            RoundCall::Pick(params) => self.pick(params).await,
            RoundCall::Start(params) => self.start(params).await,
            RoundCall::Stop(params) => self.stop(params).await,
            RoundCall::Retry(params) => self.retry(params).await,
            RoundCall::CancelAnswer(params) => {
                let offered = self
                    .round
                    .stages
                    .latest()
                    .cancellable
                    .is_some_and(|answer| answer.id == params.answer);
                let command = PageCommand::Recover(Recovery::CancelAnswer {
                    answer: params.answer,
                });
                let action = Action::Recover(RecoveryAction::CancelAnswer);
                self.offered(offered, command, action).await
            }
            RoundCall::Reset(params) => self.reset(params).await,
            RoundCall::DiagramFailed(params) => {
                let error = DiagramError {
                    question: params.question,
                    version: params.version,
                    source: params.source,
                    message: params.message,
                };
                // The page shows the error whatever the owner replies.
                let sent = self.round.commands.send(PageCommand::DiagramFailed(error));
                Ok(Outcome::applied(sent.await.unwrap_or(false)))
            }
            RoundCall::Ping => Ok(Outcome::default()),
        }
    }

    async fn on_conclusion(&self, call: ConclusionCall) -> Result<Outcome, Notice> {
        match call {
            ConclusionCall::Implement(params) => self.implement(params).await,
            ConclusionCall::Quiz(params) => self.quiz_pick(params).await,
            ConclusionCall::QuizSkip(params) => {
                let response = PageQuizResponse {
                    conclusion: params.conclusion,
                    response: QuizResponse::Skip,
                };
                self.quiz(response).await
            }
            ConclusionCall::CancelImplementation(params) => {
                let offered = self
                    .round
                    .stages
                    .stage()
                    .sends_implementation(&params.delivery);
                let command = PageCommand::Recover(Recovery::CancelImplementation {
                    delivery: params.delivery,
                });
                let action = Action::Recover(RecoveryAction::CancelImplementation);
                self.offered(offered, command, action).await
            }
            ConclusionCall::ResendImplementation(params) => self.resend(params).await,
        }
    }

    /// Hands the reviewer's answer to the round's owner, with the reviewer's first pick of a
    /// blind question, unless the page showed a question that no longer waits for an answer.
    async fn answer(&self, params: AnswerParams) -> Result<Outcome, Notice> {
        let shown = self.round.stages.latest();
        let asked = shown
            .stage
            .asks(&params.question, params.version)
            .filter(|_| params.round == shown.round);
        let comment = crlf_to_lf(params.comment);
        let action = Action::Answer {
            number: params.number,
        };
        if asked.is_none() {
            // The round's latest answer is this one: a repeat of an answer that went through.
            let answered = Answered {
                question: Some((params.question, params.version)),
                option: params.choice,
                in_reply_to: String::new(),
            };
            return repeat(shown.repeats(&answered, &comment), action);
        }
        // A question answered before, whose recommendation the reviewer has seen, keeps no first
        // pick.
        let first_pick = shown
            .stage
            .blind()
            .zip(shown.round.as_deref())
            .and_then(|(_, round)| self.page.picks.of(round, &params.question, params.version));
        let answer = PageAnswer {
            question: params.question,
            version: params.version,
            input: AnswerInput {
                option: params.choice,
                text: comment,
                in_reply_to: None,
                first_pick,
            },
        };
        self.send(PageCommand::Answer(answer), action).await
    }

    /// Keeps the reviewer's first pick of a blind question, once the round's owner says that it
    /// still asks it. A pick kept already stays the first one.
    async fn pick(&self, params: PickParams) -> Result<Outcome, Notice> {
        let action = Action::Pick {
            number: params.number,
        };
        let stale = || Notice::new(action, Problem::Stale);
        let shown = self.round.stages.latest();
        let asked = shown
            .stage
            .asks(&params.question, params.version)
            .filter(|_| params.round == shown.round);
        let (Some(_), Some(round)) = (asked, shown.round.as_deref()) else {
            return Err(stale());
        };
        let Some(blind) = shown
            .stage
            .blind()
            .filter(|blind| blind.offers(&params.choice))
        else {
            // A question that shows its recommendation at once keeps no first pick.
            return Ok(Outcome::default());
        };
        if self
            .page
            .picks
            .of(round, &params.question, params.version)
            .is_some()
        {
            return Ok(Outcome::default());
        }
        let command = PageCommand::Pick {
            question: params.question,
            version: params.version,
        };
        self.round
            .commands
            .send(command)
            .await
            .map_err(|problem| Notice::new(action, problem))?;
        let kept = self.page.picks.keep(round, &blind, &params.choice);
        Ok(Outcome::applied(kept == Pick::First))
    }

    /// Hands the start the page offered to the round's owner, unless the round offers another
    /// start since, or none, or nothing is left to review.
    async fn start(&self, params: StartParams) -> Result<Outcome, Notice> {
        let shown = self.round.stages.latest();
        if shown.stage.offered_start() != Some(params.start.as_str()) {
            let starting = matches!(&shown.stage, RoundStage::Starting { start, .. } if *start == params.start);
            return repeat(starting, Action::Start);
        }
        if let Some(block) = shown.start_block {
            return Err(Notice::new(
                Action::Start,
                Problem::Failed(block.reason().into()),
            ));
        }
        let command = PageCommand::Start {
            challenger: params.challenger,
            start: params.start,
        };
        self.send(command, Action::Start).await
    }

    /// Hands Stop waiting to the round's owner, unless the page showed a start or a turn that is
    /// no longer waited for. A repeat of a Stop waiting that stopped the agent's turn finds the
    /// turn paused.
    async fn stop(&self, params: StopParams) -> Result<Outcome, Notice> {
        let action = Action::Recover(RecoveryAction::Stop);
        let waiting = match (params.start, params.request) {
            (Some(start), None) => Waiting::Start(start),
            (None, Some(request)) => Waiting::Turn(request),
            _ => return Err(Notice::new(action, Problem::Stale)),
        };
        let stage = self.round.stages.stage();
        if !stage.stops(&waiting) {
            let paused = matches!(
                (&stage, &waiting),
                (
                    RoundStage::Interrupted { request: Some(request), interruption: Interruption::Stopped, .. },
                    Waiting::Turn(stopped),
                ) if request == stopped
            );
            return repeat(paused, action);
        }
        self.send(PageCommand::Recover(Recovery::Stop(waiting)), action)
            .await
    }

    /// Hands Retry to the round's owner, unless the page showed an attempt of the turn that is
    /// no longer its latest one. A repeat of a Retry that went through finds the agent working on
    /// the turn.
    async fn retry(&self, params: RetryParams) -> Result<Outcome, Notice> {
        let action = Action::Recover(RecoveryAction::Retry);
        let stage = self.round.stages.stage();
        if !stage.retries(&params.request, &params.attempt) {
            return repeat(stage.stops(&Waiting::Turn(params.request)), action);
        }
        let command = PageCommand::Recover(Recovery::Retry {
            request: params.request,
            attempt: params.attempt,
        });
        self.send(command, action).await
    }

    /// Hands the saved implementation request to the round's owner to send again, unless the
    /// page showed an attempt that is no longer its latest one. A repeat of a resend that went
    /// through finds the request on its way, or received.
    async fn resend(&self, params: ResendImplementationParams) -> Result<Outcome, Notice> {
        let stage = self.round.stages.stage();
        if !stage.resends_implementation(&params.conclusion, &params.delivery, &params.attempt) {
            let resent = stage.implementation().is_some_and(|implementation| {
                implementation.delivery == params.delivery
                    && matches!(
                        implementation.state,
                        ImplementationState::Sending | ImplementationState::Sent
                    )
            });
            return repeat(resent, Action::Implement);
        }
        let command = PageCommand::Recover(Recovery::ResendImplementation {
            conclusion: params.conclusion,
            delivery: params.delivery,
            attempt: params.attempt,
        });
        self.send(command, Action::Implement).await
    }

    /// Hands Reset to the round's owner, unless the page showed a round that is no longer
    /// running. A Reset ends the token of a round on the network: the page receives the token of
    /// the start screen, so that it can start the next round.
    async fn reset(&self, params: ResetParams) -> Result<Outcome, Notice> {
        let offered = self.round.stages.round().as_deref() == Some(params.round.as_str());
        let command = PageCommand::Recover(Recovery::Reset {
            round: params.round,
        });
        let mut outcome = self
            .offered(offered, command, Action::Recover(RecoveryAction::Reset))
            .await?;
        outcome.reopen = self
            .page
            .rounds()
            .after_reset()
            .map(|token| token.to_string());
        Ok(outcome)
    }

    /// Hands the reviewer's Implement to the round's owner, unless the page showed a conclusion
    /// that no longer offers it.
    async fn implement(&self, params: ImplementParams) -> Result<Outcome, Notice> {
        let stage = self.round.stages.stage();
        let text = crlf_to_lf(params.text);
        if !stage.offers_implement(&params.conclusion, params.replaces.as_deref()) {
            // The conclusion's latest request is this list, on its way or received, in place of
            // the one the page showed: a repeat of an Implement that went through.
            let sent = stage.concludes(&params.conclusion)
                && stage.implementation().is_some_and(|latest| {
                    latest.text == text
                        && !latest.state.allows_another()
                        && params.replaces.as_deref() != Some(latest.delivery.as_str())
                });
            return repeat(sent, Action::Implement);
        }
        let command = PageCommand::Implement(PageImplement {
            conclusion: params.conclusion,
            replaces: params.replaces,
            text,
        });
        self.send(command, Action::Implement).await
    }

    async fn quiz_pick(&self, params: QuizParams) -> Result<Outcome, Notice> {
        let response = PageQuizResponse {
            conclusion: params.conclusion,
            response: QuizResponse::Pick {
                item: params.item,
                answer: params.answer,
            },
        };
        self.quiz(response).await
    }

    /// Sends `response` to the round's owner, unless the round no longer shows the quiz where it
    /// fits. A repeat of a pick or a skip the round saved changes nothing.
    async fn quiz(&self, response: PageQuizResponse) -> Result<Outcome, Notice> {
        let stage = self.round.stages.stage();
        if stage.quiz_has(&response) {
            return Ok(Outcome::applied(false));
        }
        let offered = stage.takes_quiz(&response);
        self.offered(offered, PageCommand::Quiz(response), Action::Quiz)
            .await
    }

    /// Hands `command` to the round's owner when the page offers it (`offered`); `action` words
    /// the notice when it did not go through.
    async fn offered(
        &self,
        offered: bool,
        command: PageCommand,
        action: Action,
    ) -> Result<Outcome, Notice> {
        if offered {
            self.send(command, action).await
        } else {
            Err(Notice::new(action, Problem::Stale))
        }
    }

    async fn send(&self, command: PageCommand, action: Action) -> Result<Outcome, Notice> {
        match self.round.commands.send(command).await {
            Ok(applied) => Ok(Outcome::applied(applied)),
            Err(problem) => Err(Notice::new(action, problem)),
        }
    }
}

/// The reply to an action the round no longer offers: applied already when it is a `repeat` of
/// one that went through, else stale.
fn repeat(repeat: bool, action: Action) -> Result<Outcome, Notice> {
    if repeat {
        Ok(Outcome::applied(false))
    } else {
        Err(Notice::new(action, Problem::Stale))
    }
}

/// The text of a text area as the reviewer wrote it: a browser may send its line breaks as
/// CRLF, while the pane's editor keeps LF, so that the round saves the same text whichever
/// front end the reviewer wrote it in.
fn crlf_to_lf(text: String) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n")
    } else {
        text
    }
}
