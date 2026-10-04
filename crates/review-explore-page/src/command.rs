//! What the reviewer asks of a round from the page, and the reply of the round's owner.

use std::sync::Arc;
use std::time::Duration;

use review_explore::{AnswerInput, DiagramError, QuizResponse};
use tokio::sync::oneshot;

use crate::notice::Problem;

/// How long a post waits for the owner's reply. The owner handles commands in turn with its
/// other work, which a long repository refresh can hold up.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// An action the reviewer took on the page, for the round's owner.
#[derive(Debug)]
pub enum PageCommand {
    /// Answer the question the page showed.
    Answer(PageAnswer),
    /// Mermaid could not draw a diagram of the question the page showed: save the error with
    /// the question.
    DiagramFailed(DiagramError),
    /// Start a round, as Start or Start with Challenger in the pane: the page showed that no
    /// round was running.
    Start {
        /// Whether the Challenger reviews the change beside the agent.
        challenger: bool,
    },
    /// Implement the conclusion the page showed.
    Implement(PageImplement),
    /// Save the reviewer's pick of a quiz item, or skip of the quiz, of the conclusion the page
    /// showed.
    Quiz(PageQuizResponse),
    /// Reply to the conclusion the page showed, as Reply in the pane.
    Reply(PageReply),
    /// Recover or close the round, as the pane offers it.
    Recover(Recovery),
}

/// What the reviewer does on the page to recover the round, or to close it, as in the pane.
#[derive(Debug)]
pub enum Recovery {
    /// Stop waiting: for the start under way when `request` is `None`, which the page showed as
    /// starting, else for the agent's turn `request`, which the page showed the agent working
    /// on.
    Stop { request: Option<String> },
    /// Send again the agent's turn `request`, which the page showed as interrupted.
    Retry { request: String },
    /// Cancel the reviewer's latest answer, `answer`.
    CancelAnswer { answer: String },
    /// Close the round `round` that the page showed, and return to the start screen.
    Reset { round: String },
    /// Cancel the implementation request `delivery`, which the page showed as being sent.
    CancelImplementation { delivery: String },
    /// Send the implementation request `delivery` of the conclusion of the turn `conclusion`,
    /// which the page showed as saved but not sent, as "Send saved implementation request" in
    /// the pane.
    ResendImplementation {
        conclusion: String,
        delivery: String,
    },
}

/// The reviewer's reply to the conclusion the page showed: free text, with no choice. The owner
/// builds the turn from its own round.
#[derive(Debug)]
pub struct PageReply {
    /// The request of the agent's turn that posted the conclusion.
    pub conclusion: String,
    pub text: String,
}

/// What the reviewer did with the quiz of the conclusion the page showed. The page graded a
/// pick already; the owner saves it with the round.
#[derive(Debug)]
pub struct PageQuizResponse {
    /// The request of the agent's turn that posted the conclusion.
    pub conclusion: String,
    pub response: QuizResponse,
}

/// The reviewer's answer to the question the page showed: only the pick and the comment. The
/// owner builds the turn from its own round.
#[derive(Debug)]
pub struct PageAnswer {
    /// The ID of the question the page showed.
    pub question: String,
    /// The version of that question.
    pub version: u32,
    /// The picked choice, by ID, if any, and the comment.
    pub input: AnswerInput,
}

/// The reviewer's Implement of the conclusion the page showed: the list to be implemented, as
/// the reviewer edited it, and what the page showed of the conclusion's earlier request. The
/// owner builds the request from its own round.
#[derive(Debug)]
pub struct PageImplement {
    /// The request of the agent's turn that posted the conclusion.
    pub conclusion: String,
    /// The delivery of the conclusion's latest implementation request, which the page showed
    /// as not sent and the new request replaces; `None` when the page showed no request.
    pub replaces: Option<String>,
    /// The list to be implemented, as the reviewer edited it.
    pub text: String,
}

/// Why the round's owner did not carry out a command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandRefusal {
    /// The round moved on since the page was loaded, so what the page showed no longer offers
    /// the command: the question the page showed no longer waits for an answer, as it may have
    /// one already, from the pane or from another page; or a round started since the page
    /// showed none; or the conclusion the page showed has another implementation request, from
    /// the pane or from another page; or its quiz item has a pick already, from another page;
    /// or the agent's turn the page showed is no longer waited for or interrupted; or the
    /// answer the page offered to cancel is no longer the latest one.
    Stale,
    /// The owner could not carry out the command, for this reason.
    Failed(String),
}

/// The owner's reply to one command.
#[derive(Debug)]
pub struct CommandReply(oneshot::Sender<Result<(), CommandRefusal>>);

impl CommandReply {
    /// A reply, and the receiver it arrives at.
    pub fn channel() -> (Self, oneshot::Receiver<Result<(), CommandRefusal>>) {
        let (reply, replied) = oneshot::channel();
        (Self(reply), replied)
    }

    pub fn send(self, result: Result<(), CommandRefusal>) {
        // The page stopped waiting: it no longer needs the reply.
        let _ = self.0.send(result);
    }
}

/// Where a page sends the reviewer's commands for one round: to the round's owner, which
/// handles them in turn with its other work and replies to each.
#[derive(Clone)]
pub struct CommandSender(Arc<dyn Fn(PageCommand, CommandReply) + Send + Sync>);

impl CommandSender {
    pub fn new(deliver: impl Fn(PageCommand, CommandReply) + Send + Sync + 'static) -> Self {
        Self(Arc::new(deliver))
    }

    /// Sends `command` to the owner and waits for its reply: why the command did not go
    /// through, when it did not.
    pub(crate) async fn send(&self, command: PageCommand) -> Result<(), Problem> {
        let (reply, replied) = CommandReply::channel();
        (self.0)(command, reply);
        match tokio::time::timeout(REPLY_TIMEOUT, replied).await {
            Ok(Ok(result)) => result.map_err(Problem::from),
            // The owner stopped, or did not reply in time: the command may have gone through.
            Ok(Err(_)) | Err(_) => Err(Problem::NoReply),
        }
    }
}
