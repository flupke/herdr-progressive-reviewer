//! The status card, as data: what the page shows for every state that is not a question. Each
//! state maps here, and only here, to a kind, a title that says the state, the reason, the next
//! step and the actions that fit it (docs/design/explore-page/README.md, "Start cover and status
//! cards"; design-review.md, findings 6 and 23). The action names are the pane's. The page's
//! client draws a card from these fields (`assets/client/status.js`); a refused action's card
//! travels in the socket's reply.

use review_explore::StartBlock;
use serde::Serialize;
use ts_rs::TS;

use crate::notice::{Action, Notice, Problem, RecoveryAction};
use crate::round::{
    ImplementationState, Interruption, PageImplementation, RoundSnapshot, RoundStage,
};

/// What a card tells, by its colour and glyph.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
enum StatusKind {
    /// The review tool or the agent is at work: a moving bar.
    Progress,
    /// A fact the reviewer should know; nothing is wrong.
    Info,
    /// Check something before you act.
    Warn,
    /// Something failed.
    Danger,
    /// Done, with nothing left to do.
    Ok,
}

/// How a reader is told of a card.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
enum StatusRole {
    /// A state the page shows.
    Status,
    /// A failure, or a refusal of the reviewer's action, which the reviewer has to see at once.
    Alert,
    /// A lasting fact about the round.
    Note,
}

/// The tier of an action's button (assets/buttons.css).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
enum ButtonTier {
    Primary,
    Secondary,
}

/// One state that is not a question: the state first, then the next step.
#[derive(Debug, Serialize, TS)]
pub(crate) struct StatusCard {
    kind: StatusKind,
    /// Names the card, once on the page.
    id: &'static str,
    role: StatusRole,
    title: String,
    reason: Option<String>,
    /// Whether the reason is a verbatim error, shown as code.
    code: bool,
    /// A next step the reviewer must not miss, in bold before `next`.
    imperative: Option<&'static str>,
    next: Option<&'static str>,
    /// In a row under the text, primary first.
    actions: Vec<StatusAction>,
}

/// An action of a card: a request the page sends.
#[derive(Debug, Serialize, TS)]
struct StatusAction {
    /// The method of the page's request, which also names its form: `retry` sends a Retry.
    method: &'static str,
    /// The identities the request carries.
    fields: Vec<Field>,
    label: &'static str,
    tier: ButtonTier,
    /// A short line beside the button.
    hint: Option<&'static str>,
}

#[derive(Debug, Serialize, TS)]
struct Field {
    name: &'static str,
    value: String,
}

impl StatusCard {
    fn new(kind: StatusKind, id: &'static str, title: impl Into<String>) -> Self {
        Self {
            kind,
            id,
            role: StatusRole::Status,
            title: title.into(),
            reason: None,
            code: false,
            imperative: None,
            next: None,
            actions: Vec::new(),
        }
    }

    fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    fn code(mut self) -> Self {
        self.code = true;
        self
    }

    fn next(mut self, next: &'static str) -> Self {
        self.next = Some(next);
        self
    }

    fn imperative(mut self, imperative: &'static str) -> Self {
        self.imperative = Some(imperative);
        self
    }

    fn role(mut self, role: StatusRole) -> Self {
        self.role = role;
        self
    }

    fn action(mut self, action: Option<StatusAction>) -> Self {
        self.actions.extend(action);
        self
    }

    /// The next step `next`, when `shown`.
    fn next_if(self, shown: bool, next: &'static str) -> Self {
        if shown { self.next(next) } else { self }
    }

    /// Retry of the agent's turn `turn`, its request and its latest attempt, with its next
    /// step, when there is a turn to send again.
    fn retry(
        self,
        turn: Option<(&str, &str)>,
        next: Option<&'static str>,
        tier: ButtonTier,
        hint: Option<&'static str>,
    ) -> Self {
        let Some((request, attempt)) = turn else {
            return self;
        };
        let card = match next {
            Some(next) => self.next(next),
            None => self,
        };
        card.action(Some(StatusAction::retry(request, attempt, tier, hint)))
    }

    /// The ID of the card in `cards` that says why no round can start, if any.
    pub(crate) fn start_block(cards: &[Self]) -> Option<&'static str> {
        cards
            .iter()
            .map(|card| card.id)
            .find(|id| *id == START_BLOCK)
    }

    /// Whether a card in `cards` says the start cover's state: a failed start, or nothing left
    /// to review.
    pub(crate) fn say_start(cards: &[Self]) -> bool {
        cards
            .iter()
            .any(|card| [START_FAILURE, START_BLOCK].contains(&card.id))
    }

    /// The cards above the round's stage: that the round is an earlier one, then the stage's
    /// own cards. `offers_actions` tells whether the page offers actions on the round: an
    /// earlier round offers Reset only, and shows its own card in place of the stage's.
    pub(crate) fn above_stage(round: &RoundSnapshot, offers_actions: bool) -> Vec<Self> {
        let mut cards = Vec::new();
        if !offers_actions {
            cards.push(Self::earlier());
        }
        cards.extend(Self::of_stage(round, offers_actions));
        cards
    }

    /// The card of the conclusion's latest implementation request, with the action that
    /// recovers it. `request` is the agent's turn that posted the conclusion; `offers_actions`
    /// tells whether the page offers actions on the conclusion, `offers_implement` whether it
    /// offers a new request.
    pub(crate) fn implementation(
        implementation: &PageImplementation,
        request: &str,
        offers_actions: bool,
        offers_implement: bool,
    ) -> Self {
        use StatusKind::{Danger, Info, Ok, Progress, Warn};
        const ID: &str = "implementation";
        let delivery = || Field::new("delivery", &implementation.delivery);
        // Sends the request again, as it was saved.
        let resend = |label| StatusAction {
            method: "resend-implementation",
            fields: vec![
                Field::new("conclusion", request),
                delivery(),
                Field::new("attempt", &implementation.attempt),
            ],
            label,
            tier: ButtonTier::Primary,
            hint: None,
        };
        match &implementation.state {
            ImplementationState::Sending => Self::new(
                Progress,
                ID,
                "Sending the implementation request to the agent…",
            )
            .action(offers_actions.then(|| StatusAction {
                method: "cancel-implementation",
                fields: vec![delivery()],
                label: "Cancel the implementation request",
                tier: ButtonTier::Secondary,
                hint: Some("Cancel stops the request if it has not reached the agent yet."),
            })),
            ImplementationState::Sent => {
                Self::new(Ok, ID, "The agent received the implementation request")
            }
            ImplementationState::NotStarted => Self::new(
                Warn,
                ID,
                "The agent did not start on the implementation request",
            )
            .reason("The list may still wait in the agent's prompt box.")
            .role(StatusRole::Alert)
            .next_if(
                offers_actions,
                "Look at the agent's pane, then Retry: it sends the same request again.",
            )
            .action(offers_actions.then(|| resend("Retry"))),
            ImplementationState::Paused => Self::new(Info, ID, "The request was never sent")
                .reason(
                    "The review pane was reopened before it went out. The list you authorized \
                     is saved.",
                )
                .next_if(
                    offers_implement,
                    "Send it as it was saved, or edit the list and send a new one.",
                )
                .action(offers_implement.then(|| resend("Send the saved request"))),
            ImplementationState::Unknown => {
                Self::new(Warn, ID, "The agent may or may not have the request")
                    .reason("The review pane was reopened while it sent the request.")
                    .imperative("Check the agent's conversation first.")
                    .next_if(offers_implement, "Send again only if it never arrived.")
            }
            ImplementationState::NotSent(reason) => {
                Self::new(Danger, ID, "The implementation request could not be sent")
                    .reason(reason.as_str())
                    .role(StatusRole::Alert)
            }
            ImplementationState::Cancelled => Self::new(
                Info,
                ID,
                "The implementation request was cancelled before it was sent",
            ),
        }
    }

    /// Why the reviewer's latest action did not go through. An action the round moved past is
    /// information; one that failed shows the failure; one the review tool did not answer asks
    /// the reviewer to check.
    pub(crate) fn of_notice(notice: &Notice) -> Self {
        let words = NoticeWords::of(notice.action);
        let card = match &notice.problem {
            Problem::Stale => Self::new(StatusKind::Info, "notice", words.moved)
                .reason(words.stale)
                .next("This page now shows the round as it is."),
            Problem::Failed(reason) => {
                Self::new(StatusKind::Danger, "notice", words.title).reason(reason.as_str())
            }
            Problem::NoReply => {
                Self::new(StatusKind::Warn, "notice", "The review tool did not reply")
                    .next(words.unknown)
            }
        };
        card.role(StatusRole::Alert)
    }

    fn earlier() -> Self {
        Self::new(StatusKind::Info, "earlier", "This is an earlier round")
            .reason("Another review pane saved a newer round of this review.")
            .next("Reopen the review pane to see the newer round, or reset this one to start over.")
            .role(StatusRole::Note)
    }

    /// The cards of a stage that is not a question: the start under way or failed, the agent's
    /// turn under way or interrupted, a storage failure; and on the start cover, that nothing
    /// is left to review. `offers_actions` tells whether the page offers actions on the round.
    fn of_stage(round: &RoundSnapshot, offers_actions: bool) -> Vec<Self> {
        let answered = round.cancellable.is_some() && offers_actions;
        match &round.stage {
            RoundStage::NoRound { .. } => Self::start(round, None),
            RoundStage::StartFailed { failure, .. } => Self::start(round, Some(failure)),
            RoundStage::Starting { start } => vec![Self::starting(start)],
            RoundStage::AgentWorking { request } => {
                vec![Self::working(round, request, answered, offers_actions)]
            }
            RoundStage::Interrupted {
                request,
                attempt,
                interruption,
            } => offers_actions
                .then(|| {
                    let turn = request.as_deref().zip(attempt.as_deref());
                    Self::interrupted(request.is_some(), turn, interruption, answered)
                })
                .into_iter()
                .collect(),
            RoundStage::StorageFailed { failure } => vec![Self::storage(failure)],
            RoundStage::Question { .. } | RoundStage::Conclusion { .. } => Vec::new(),
        }
    }

    /// The cards of the start cover: why the reviewer's latest start failed, when it did, and
    /// that nothing is left to review. A start that failed because nothing is left to review
    /// shows only that good news.
    fn start(round: &RoundSnapshot, failure: Option<&str>) -> Vec<Self> {
        let failed = failure
            .filter(|failure| {
                round
                    .start_block
                    .is_none_or(|block| block.reason() != *failure)
            })
            .map(|failure| {
                Self::new(
                    StatusKind::Danger,
                    START_FAILURE,
                    "The round could not start",
                )
                .reason(failure)
                .role(StatusRole::Alert)
            });
        let blocked = round.start_block.map(|block| match block {
            StartBlock::NothingToReview => Self::new(
                StatusKind::Ok,
                START_BLOCK,
                "Every changed line is reviewed",
            )
            .next(
                "Nothing is left to explore. Unmark lines or files in the review pane to \
                     start a round.",
            ),
        });
        failed.into_iter().chain(blocked).collect()
    }

    /// The start `start` is under way.
    fn starting(start: &str) -> Self {
        Self::new(StatusKind::Progress, "waiting", "Preparing the round")
            .reason(
                "The review tool captures the change; Jev marks the insignificant lines first \
                 when it is on.",
            )
            .action(Some(StatusAction::stop(Stopped::Start(start), false)))
    }

    /// The agent works on its turn `request`; `answered` tells whether the turn carries the
    /// reviewer's answer, `offers_actions` whether the page offers Stop waiting.
    fn working(round: &RoundSnapshot, request: &str, answered: bool, offers_actions: bool) -> Self {
        let title = if answered {
            "The agent is working on your answer"
        } else if round.design.is_none() {
            "The agent is working on the design and its first question"
        } else {
            "The agent is working on its next turn"
        };
        Self::new(StatusKind::Progress, "waiting", title)
            .action(offers_actions.then(|| StatusAction::stop(Stopped::Turn(request), answered)))
    }

    fn storage(failure: &str) -> Self {
        Self::new(
            StatusKind::Danger,
            "storage",
            "Explore cannot save rounds, so this page can do nothing",
        )
        .reason(failure)
        .code()
        .next("Fix the problem, then close the review pane and open it again.")
        .role(StatusRole::Alert)
    }

    /// The agent is not working on the turn the round waits for, if `any`; Retry sends `turn`
    /// again, its request and latest attempt. `answered` tells whether the turn carries the
    /// reviewer's answer.
    fn interrupted(
        any: bool,
        turn: Option<(&str, &str)>,
        interruption: &Interruption,
        answered: bool,
    ) -> Self {
        use StatusKind::{Danger, Info, Warn};
        const ID: &str = "interruption";
        let (subject, what) = if answered {
            ("Your answer", "your answer")
        } else {
            ("The turn's prompt", "the turn's prompt")
        };
        match interruption {
            Interruption::Failed(failure) => {
                Self::new(Danger, ID, format!("{subject} did not reach the agent"))
                    .reason(failure.as_str())
                    .role(StatusRole::Alert)
                    .retry(
                        turn,
                        Some("Look at the review pane, then Retry."),
                        ButtonTier::Primary,
                        answered.then_some("Your answer and its marks are kept."),
                    )
            }
            Interruption::NotStarted => {
                Self::new(Warn, ID, format!("The agent did not start on {what}"))
                    .reason("Its prompt may still wait in the agent's prompt box.")
                    .role(StatusRole::Alert)
                    .retry(
                        turn,
                        Some("Look at the agent's pane, then Retry: it sends the same turn again."),
                        ButtonTier::Primary,
                        None,
                    )
            }
            Interruption::Uncertain => {
                Self::new(Warn, ID, format!("The agent may or may not have {what}"))
                    .reason("The review pane was reopened while it sent the prompt.")
                    .imperative("Check the agent's conversation before you retry.")
                    .retry(
                        turn,
                        None,
                        ButtonTier::Secondary,
                        Some("Sends the same turn again, which could duplicate it."),
                    )
            }
            Interruption::Stopped if !any => {
                Self::new(Info, ID, "The round has no turn to send again")
                    .next("Reset the round, then start a new one.")
            }
            Interruption::Stopped => Self::new(Info, ID, "The turn is paused")
                .reason(
                    "You stopped waiting, or the review pane was reopened before the prompt went \
                     out.",
                )
                .retry(
                    turn,
                    Some(if answered {
                        "Retry sends it again, with your answer."
                    } else {
                        "Retry sends it again."
                    }),
                    ButtonTier::Primary,
                    None,
                ),
        }
    }
}

impl StatusAction {
    /// Stop waiting for `stopped`; `answered` tells whether the agent's turn carries the
    /// reviewer's answer.
    fn stop(stopped: Stopped<'_>, answered: bool) -> Self {
        let (hint, field) = match stopped {
            Stopped::Turn(request) if answered => (
                "Your answer stays; Retry sends it again.",
                Field::new("request", request),
            ),
            Stopped::Turn(request) => (
                "After Stop waiting, Retry sends the turn again.",
                Field::new("request", request),
            ),
            Stopped::Start(start) => (
                "Stop waiting drops the round before it starts.",
                Field::new("start", start),
            ),
        };
        Self {
            method: "stop",
            fields: vec![field],
            label: "Stop waiting",
            tier: ButtonTier::Secondary,
            hint: Some(hint),
        }
    }

    /// Retry of the attempt `attempt` of the agent's turn `request`.
    fn retry(request: &str, attempt: &str, tier: ButtonTier, hint: Option<&'static str>) -> Self {
        Self {
            method: "retry",
            fields: vec![
                Field::new("request", request),
                Field::new("attempt", attempt),
            ],
            label: "Retry",
            tier,
            hint,
        }
    }
}

/// What a Stop waiting stops: a start, or an agent's turn.
#[derive(Clone, Copy)]
enum Stopped<'a> {
    Start(&'a str),
    Turn(&'a str),
}

impl Field {
    fn new(name: &'static str, value: &str) -> Self {
        Self {
            name,
            value: value.to_owned(),
        }
    }
}

/// How a notice words an action that did not go through.
struct NoticeWords {
    /// What did not happen, when the action failed.
    title: &'static str,
    /// The state first, when the round moved past the action.
    moved: &'static str,
    /// Then what it meant for the action.
    stale: &'static str,
    /// What to do when the review tool did not reply: the action may have gone through.
    unknown: &'static str,
}

impl NoticeWords {
    const fn new(
        title: &'static str,
        moved: &'static str,
        stale: &'static str,
        unknown: &'static str,
    ) -> Self {
        Self {
            title,
            moved,
            stale,
            unknown,
        }
    }

    fn of(action: Action) -> Self {
        match action {
            Action::Answer => Self::new(
                "Your answer was not sent",
                QUESTION_ANSWERED,
                "In the review pane or in another tab, or the round moved on, so your answer was \
                 not sent.",
                "Load this page again to see whether it took your answer.",
            ),
            Action::Start => Self::new(
                "The round was not started",
                "A round was started meanwhile",
                "In the pane or in another tab, so this start did nothing.",
                "Load this page again to see whether it started the round.",
            ),
            Action::Pick => Self::new(
                "Your pick was not kept",
                QUESTION_ANSWERED,
                "In the review pane or in another tab, or the round moved on, so your pick was not \
                 kept.",
                "Load this page again to see whether it kept your pick.",
            ),
            Action::Implement => Self::new(
                "The implementation request was not sent",
                "This conclusion no longer waits for a request",
                "A request was sent from the pane or from another tab, or the round moved on, so \
                 this one was not sent.",
                "Load this page again to see whether it sent the implementation request.",
            ),
            Action::Quiz => Self::new(
                "Your quiz answer was not kept",
                "This quiz question no longer waits for an answer",
                "It was answered or skipped in another tab, or the round moved on, so your quiz \
                 answer was not kept.",
                "Load this page again to see whether it kept your quiz answer.",
            ),
            Action::Reply => Self::new(
                "Your reply was not sent",
                "The round moved past this conclusion",
                "In the pane or in another tab, so your reply was not sent.",
                "Load this page again to see whether it took your reply.",
            ),
            Action::Recover(recovery) => Self::recovery(recovery),
        }
    }

    /// How a notice words an action that recovers or closes the round.
    fn recovery(action: RecoveryAction) -> Self {
        match action {
            RecoveryAction::Stop => Self::new(
                "Stop waiting did nothing",
                "The page no longer waits for what it showed",
                "The round moved on, in the pane or in another tab, so Stop waiting did nothing.",
                "Load this page again to see whether it stopped waiting.",
            ),
            RecoveryAction::Retry => Self::new(
                "Retry did nothing",
                "The turn is no longer interrupted",
                "It was sent again in the pane or in another tab, or the round moved on, so Retry \
                 did nothing.",
                "Load this page again to see whether it sent the turn again.",
            ),
            RecoveryAction::CancelAnswer => Self::new(
                "Your answer was not cancelled",
                "It is no longer your last answer",
                "The round moved on, in the pane or in another tab, so your answer was not \
                 cancelled.",
                "Load this page again to see whether it cancelled your answer.",
            ),
            RecoveryAction::Reset => Self::new(
                "The round was not reset",
                "The round was already reset or replaced",
                "In the pane or in another tab, so this reset did nothing.",
                "Load this page again to see whether it reset the round.",
            ),
            RecoveryAction::CancelImplementation => Self::new(
                "The implementation request was not cancelled",
                "The implementation request is no longer being sent",
                "It was sent or cancelled meanwhile, so it was not cancelled.",
                "Load this page again to see whether it cancelled the implementation request.",
            ),
        }
    }
}

/// The IDs of the start cover's cards.
const START_FAILURE: &str = "start-failure";
const START_BLOCK: &str = "start-block";

/// The state of a question whose answer or first pick the page refused.
const QUESTION_ANSWERED: &str = "This question was already answered";
