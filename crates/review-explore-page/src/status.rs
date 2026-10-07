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
    /// When the state began, which the reason opens with, in the reader's clock.
    time: Option<CardTime>,
    reason: Option<String>,
    /// When what the card waits for began, which the reason ends with as the time since then,
    /// counted in the page: "Sent 0:42 ago".
    since: Option<CardTime>,
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

/// A time a card's reason tells: "Sent at 14:36.", in the reader's clock, or "Sent 0:42 ago",
/// counted in the page.
#[derive(Debug, Serialize, TS)]
struct CardTime {
    /// What happened at that time: "Sent at", or "Sent" before the time since then.
    words: &'static str,
    /// Milliseconds since the epoch.
    ms: u64,
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
            time: None,
            reason: None,
            since: None,
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

    fn time(mut self, words: &'static str, ms: Option<u64>) -> Self {
        self.time = ms.map(|ms| CardTime { words, ms });
        self
    }

    fn since(mut self, words: &'static str, ms: Option<u64>) -> Self {
        self.since = ms.map(|ms| CardTime { words, ms });
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
                    .time("Sent at", implementation.sent_at_ms)
                    .reason("It implements the list; this round is done.")
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
                .reason("The pane closed before it went out. The list you authorized is saved.")
                .action(offers_implement.then(|| resend("Send the saved request"))),
            ImplementationState::Unknown => {
                Self::new(Warn, ID, "The agent may or may not have the request")
                    .reason("The review pane was reopened while it sent the request.")
                    .imperative("Check the agent's pane first.")
                    .next_if(offers_implement, "Send again only if it never arrived.")
                    .action(offers_implement.then(|| StatusAction {
                        method: "implement",
                        fields: vec![
                            Field::new("conclusion", request),
                            Field::new("replaces", &implementation.delivery),
                            Field::new("text", &implementation.text),
                        ],
                        label: "Send a new request anyway",
                        tier: ButtonTier::Secondary,
                        hint: None,
                    }))
            }
            ImplementationState::NotSent(reason) => {
                Self::new(Danger, ID, "The implementation request could not be sent")
                    .reason(reason.as_str())
                    .code()
                    .next_if(
                        offers_implement,
                        "Select an agent in the review pane, then Implement again.",
                    )
                    .role(StatusRole::Alert)
            }
            ImplementationState::Cancelled => Self::new(
                Info,
                ID,
                "The implementation request was cancelled before it was sent",
            ),
        }
    }

    /// That the agent did not get the reviewer's waiting messages in the conversation of the
    /// round `round`, because the wakeup that carried them failed with `error`: Retry wakes it
    /// again, as Retry agent does for a thread in the pane.
    pub(crate) fn undelivered_messages(error: &str, round: &str) -> Self {
        Self::new(
            StatusKind::Danger,
            "conversation-delivery",
            "Your message did not reach the agent",
        )
        .reason(error)
        .code()
        .next(SELECT_AGENT_THEN_RETRY)
        .role(StatusRole::Alert)
        .action(Some(StatusAction {
            method: "retry-messages",
            fields: vec![Field::new("round", round)],
            label: "Retry",
            tier: ButtonTier::Primary,
            hint: Some("Wakes the agent again for your waiting messages."),
        }))
    }

    /// Why the reviewer's latest action did not go through. An action the round moved past is
    /// information; one that failed shows the failure; one the review tool did not answer asks
    /// the reviewer to check.
    pub(crate) fn of_notice(notice: &Notice) -> Self {
        let words = NoticeWords::of(notice.action);
        let card = match &notice.problem {
            Problem::Stale => Self::new(
                StatusKind::Info,
                "notice",
                notice.action.question_number().map_or_else(
                    || words.moved.to_owned(),
                    |number| format!("Question {number} was already answered"),
                ),
            )
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
            .next(
                "Reopen the review pane to see the newer round, or reset this one from the ⋯ menu \
                 to start over.",
            )
            .role(StatusRole::Note)
    }

    /// The cards of a stage that is not a question: the start under way or failed, the agent's
    /// turn under way or interrupted, a storage failure; and on the start cover, that nothing
    /// is left to review. `offers_actions` tells whether the page offers actions on the round.
    /// The card of a turn that carries the reviewer's answer shows beside that answer instead
    /// ([`Self::of_turn`]).
    fn of_stage(round: &RoundSnapshot, offers_actions: bool) -> Vec<Self> {
        match &round.stage {
            RoundStage::NoRound { .. } => Self::start(round, None),
            RoundStage::StartFailed { failure, .. } => Self::start(round, Some(failure)),
            RoundStage::Starting {
                start,
                started_at_ms,
            } => vec![Self::starting(start, *started_at_ms)],
            RoundStage::AgentWorking { .. } | RoundStage::Interrupted { .. } => {
                if round.stage.sent().is_some() && offers_actions {
                    Vec::new()
                } else {
                    Self::of_turn(round, offers_actions).into_iter().collect()
                }
            }
            RoundStage::StorageFailed { failure } => vec![Self::storage(failure)],
            RoundStage::Question { .. } | RoundStage::Conclusion { .. } => Vec::new(),
        }
    }

    /// The card of the agent's turn the round waits for, which it works on or which waits for
    /// Retry, with the action that fits; `None` in another stage, and for an interrupted turn
    /// when the page offers no action on the round. `offers_actions` tells whether the page
    /// offers actions on the round.
    pub(crate) fn of_turn(round: &RoundSnapshot, offers_actions: bool) -> Option<Self> {
        let sent = Sent::of(round);
        match &round.stage {
            RoundStage::AgentWorking {
                request,
                sent_at_ms,
                ..
            } => Some(Self::working(
                round,
                request,
                *sent_at_ms,
                sent,
                offers_actions,
            )),
            RoundStage::Interrupted {
                request,
                attempt,
                interruption,
                ..
            } => offers_actions.then(|| {
                let turn = request.as_deref().zip(attempt.as_deref());
                Self::interrupted(request.is_some(), turn, interruption, sent)
            }),
            _ => None,
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
                .code()
                .next("Fix what it says, then Start again.")
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

    /// The start `start`, started at `started_at_ms`, is under way.
    fn starting(start: &str, started_at_ms: Option<u64>) -> Self {
        Self::new(StatusKind::Progress, "waiting", "Preparing the round")
            .reason(
                "The review tool captures the change; Jev marks the insignificant lines first \
                 when it is on.",
            )
            .since("Sent", started_at_ms)
            .action(Some(StatusAction::stop(Stopped::Start(start))))
    }

    /// The agent works on its turn `request`, whose latest attempt went out at `sent_at_ms`;
    /// `sent` tells what the turn carries of the reviewer's, `offers_actions` whether the page
    /// offers Stop waiting.
    fn working(
        round: &RoundSnapshot,
        request: &str,
        sent_at_ms: Option<u64>,
        sent: Option<Sent>,
        offers_actions: bool,
    ) -> Self {
        let title = match sent {
            Some(sent) => format!("The agent is working on {}", sent.named()),
            None if round.design.is_none() => {
                "The agent is working on the design and its first question".to_owned()
            }
            None => "The agent is working on its next turn".to_owned(),
        };
        let next = if sent.is_some_and(Sent::answers_question) {
            "You can leave this tab; its title changes when the next question is ready."
        } else {
            "You can leave this tab; its title changes when the agent's turn is ready."
        };
        Self::new(StatusKind::Progress, "waiting", title)
            .since("Sent", sent_at_ms)
            .next(next)
            .action(offers_actions.then(|| StatusAction::stop(Stopped::Turn(request, sent))))
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
    /// again, its request and latest attempt. `sent` tells what the turn carries of the
    /// reviewer's.
    fn interrupted(
        any: bool,
        turn: Option<(&str, &str)>,
        interruption: &Interruption,
        sent: Option<Sent>,
    ) -> Self {
        use StatusKind::{Danger, Info, Warn};
        const ID: &str = "interruption";
        let (subject, what) = match sent {
            Some(sent) => (sent.subject(), sent.noun()),
            None => ("The turn's prompt", "the turn's prompt"),
        };
        match interruption {
            Interruption::Failed(failure) => {
                Self::new(Danger, ID, format!("{subject} did not reach the agent"))
                    .reason(failure.as_str())
                    .role(StatusRole::Alert)
                    .retry(
                        turn,
                        Some(SELECT_AGENT_THEN_RETRY),
                        ButtonTier::Primary,
                        sent.filter(|sent| sent.answers_question())
                            .map(|_| "Your answer and its marks are kept."),
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
                    .imperative("Check the agent's pane before you retry.")
                    .retry(
                        turn,
                        None,
                        ButtonTier::Secondary,
                        Some("Sends the same turn again, which could duplicate it."),
                    )
            }
            Interruption::Stopped if !any => {
                Self::new(Info, ID, "The round has no turn to send again")
                    .next("Reset the round from the ⋯ menu, then start a new one.")
            }
            Interruption::Stopped => Self::new(Info, ID, "The turn is paused")
                .reason(
                    "You stopped waiting, or the review pane was reopened before the prompt went \
                     out.",
                )
                .retry(
                    turn,
                    Some(match sent {
                        Some(Sent::Answer { .. }) => "Retry sends it again, with your answer.",
                        Some(Sent::Reply) => "Retry sends it again, with your reply.",
                        None => "Retry sends it again.",
                    }),
                    ButtonTier::Primary,
                    None,
                ),
        }
    }
}

impl StatusAction {
    /// Stop waiting for `stopped`.
    fn stop(stopped: Stopped<'_>) -> Self {
        let (hint, field) = match stopped {
            Stopped::Turn(request, Some(Sent::Answer { .. })) => (
                "Your answer stays; Retry sends it again.",
                Field::new("request", request),
            ),
            Stopped::Turn(request, Some(Sent::Reply)) => (
                "Your reply stays; Retry sends it again.",
                Field::new("request", request),
            ),
            Stopped::Turn(request, None) => (
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

/// What an agent's turn carries of the reviewer's, as a card names it.
#[derive(Clone, Copy)]
enum Sent {
    /// An answer to a question, of this number on the rail when known.
    Answer { number: Option<usize> },
    /// A reply to the conclusion.
    Reply,
}

impl Sent {
    /// What the agent's turn of `round` carries of the reviewer's, if anything.
    fn of(round: &RoundSnapshot) -> Option<Self> {
        let answer = round.stage.sent()?;
        Some(match answer.question {
            Some(_) => Self::Answer {
                number: round.sent_number(),
            },
            None => Self::Reply,
        })
    }

    fn answers_question(self) -> bool {
        matches!(self, Self::Answer { .. })
    }

    /// "your answer", "your reply".
    fn noun(self) -> &'static str {
        match self {
            Self::Answer { .. } => "your answer",
            Self::Reply => "your reply",
        }
    }

    /// "Your answer", "Your reply", to open a sentence.
    fn subject(self) -> &'static str {
        match self {
            Self::Answer { .. } => "Your answer",
            Self::Reply => "Your reply",
        }
    }

    /// "your answer to question 2", or the noun alone when the number is not known.
    fn named(self) -> String {
        match self {
            Self::Answer {
                number: Some(number),
            } => format!("your answer to question {number}"),
            _ => self.noun().to_owned(),
        }
    }
}

/// What a Stop waiting stops: a start, or an agent's turn.
#[derive(Clone, Copy)]
enum Stopped<'a> {
    Start(&'a str),
    /// The agent's turn, by its request, with what it carries of the reviewer's.
    Turn(&'a str, Option<Sent>),
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
            Action::Answer { .. } => Self::new(
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
            Action::Pick { .. } => Self::new(
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
            Action::Message => Self::new(
                "Your message was not sent",
                "This round is over",
                "It was reset or replaced, in the pane or in another tab, so your message was not \
                 sent.",
                "Load this page again to see whether the conversation took your message.",
            ),
            Action::RetryMessages => Self::new(
                "Retry did nothing",
                "No message waits for the agent",
                "The agent replied, or the round moved on, so Retry did nothing.",
                "Load this page again to see whether the agent was woken again.",
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

/// The next step after a prompt that did not reach the agent (design review, finding 23).
const SELECT_AGENT_THEN_RETRY: &str = "Select an agent in the review pane, then Retry.";

/// The IDs of the start cover's cards.
const START_FAILURE: &str = "start-failure";
const START_BLOCK: &str = "start-block";

/// The state of a question whose answer or first pick the page refused.
const QUESTION_ANSWERED: &str = "This question was already answered";
