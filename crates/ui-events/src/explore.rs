use review_explore::{Comparison, EvidenceRef, InterviewUpdate};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ExploreCaptured {
    pub result: Result<Arc<Comparison>, String>,
}

/// Local restoration does not dispatch a prompt or replay agent output as new events.
#[derive(Clone, Debug)]
pub struct ExploreRestored {
    pub result: Result<Option<Arc<review_explore::ExploreRound>>, String>,
    pub view: Option<review_explore::ViewSave>,
    pub historical: bool,
    /// Ancillary editor damage does not prevent reading intact accepted history.
    pub storage_error: Option<String>,
    /// Where the restored interview stands; reopening never sends anything itself.
    pub progress: ExploreProgress,
}

/// Where a restored interview stands, decided by the Explore session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExploreProgress {
    /// No turn is waiting; the reviewer can answer or start a new round.
    #[default]
    Ready,
    /// A posted turn was not answered; Retry sends it again.
    Interrupted,
    /// A posted turn's prompt may already have reached the agent.
    DeliveryUncertain,
    /// The agent did not start on a posted turn's prompt; Retry sends it again.
    NotStarted,
}

#[derive(Clone, Debug)]
pub struct ExplorePosted {
    pub request: review_explore::TurnRequest,
    pub result: Result<Arc<review_explore::ExploreRound>, String>,
}

#[derive(Clone, Debug)]
pub struct ExploreCommitted {
    pub round: Arc<review_explore::ExploreRound>,
    pub applied: bool,
    pub response: std::sync::mpsc::Sender<Result<bool, String>>,
}

/// The reviewer's latest answer was cancelled, or why it could not be.
#[derive(Clone, Debug)]
pub struct ExploreAnswerCancelled {
    pub answer: String,
    /// The round without the answer and the agent's turn after it.
    pub result: Result<Arc<review_explore::ExploreRound>, String>,
}

#[derive(Clone, Debug)]
pub struct ExploreStorageFailed(pub String);

/// The address of the Explore page on the network, with its token, for the pane's QR code: the
/// running round's page, or while no round runs, the page that starts the next one. Never
/// sent when the page is not on the network.
#[derive(Clone, Debug)]
pub struct ExplorePageShared(pub String);

/// The Explore page could not be served on the network, for this reason: the pane says so where
/// the address and QR code would be. Never sent when the settings keep the page on this machine.
#[derive(Clone, Debug)]
pub struct ExplorePageNotShared(pub String);

/// What Start and Start with Challenger do in the pane, as the settings say. Sent once, when the
/// reviewer starts.
#[derive(Clone, Debug)]
pub struct ExplorePaneStarts(pub review_explore_page_opening::PaneStarts);

/// The browser could not open the Explore page the pane asked for: the pane says why and shows
/// the page's address, when the page has one.
#[derive(Clone, Debug)]
pub struct ExplorePageNotOpened(pub review_explore_page_opening::PageNotOpened);

/// A round the reviewer started on the Explore page: it is starting (`Ok`), or it could not
/// start, for this reason. Once its kickoff is saved, `ExplorePosted` brings the round.
#[derive(Clone, Debug)]
pub struct ExplorePageStart(pub Result<(), String>);

/// Whether the reviewer can start a round on the review: why not, when the review marks leave
/// nothing to review, or `None` once a round can start again. Sent when it changes.
#[derive(Clone, Debug)]
pub struct ExploreStartBlock(pub Option<review_explore::StartBlock>);

/// The reviewer stopped waiting on the Explore page: for the agent's turn of the round `round`,
/// or for the start under way when `round` is `None`. The pane follows it as its own Stop
/// waiting.
#[derive(Clone, Debug)]
pub struct ExplorePageStopped {
    pub round: Option<String>,
}

/// The reviewer reset the round `round` on the Explore page: the pane shows its start screen.
#[derive(Clone, Debug)]
pub struct ExplorePageReset {
    pub round: String,
}

#[derive(Clone, Debug)]
pub struct ExploreImplementationSaved(pub review_explore::ImplementationDelivery);

/// The interview accepted this capture; cancelled completions never reach viewers.
#[derive(Clone, Debug)]
pub struct ExploreComparisonAccepted(pub Arc<Comparison>);

#[derive(Clone, Debug)]
pub struct ExploreFinished {
    pub instance: String,
    pub request: String,
    pub result: Result<InterviewUpdate, String>,
}

/// The UI's serial interview owner acknowledges only a validated, applied MCP result.
#[derive(Clone, Debug)]
pub struct ExploreSubmission {
    pub update: InterviewUpdate,
    pub response: std::sync::mpsc::Sender<Result<bool, String>>,
}

/// Delivery confirmation, not a claim that the requested work has been implemented.
#[derive(Clone, Debug)]
pub struct ExploreImplementationFinished {
    pub request: review_explore::ImplementationRequest,
    /// Absent only when preparation failed before an attempt was authorized.
    pub attempt: Option<String>,
    pub state: review_explore::DispatchState,
}

#[derive(Clone, Debug)]
pub struct ExploreEvidence {
    pub comparison: Arc<Comparison>,
    pub evidence: Vec<EvidenceRef>,
    pub view: EvidenceView,
    pub reveal: bool,
}

/// One evidence reference in an immutable displayed question version.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct EvidenceView {
    pub turn: usize,
    pub reference: usize,
}

#[derive(Clone, Debug)]
pub struct ExploreViewports(pub Vec<(EvidenceView, crate::DiffViewportChanged)>);

#[derive(Clone, Debug)]
pub struct ExploreEvidenceInput {
    pub view: EvidenceView,
    pub input: crate::PointerInput,
}

#[derive(Clone, Debug)]
pub struct ExplorePositionsRestored(pub Vec<review_explore::EvidencePosition>);

#[derive(Clone, Debug)]
pub struct ExploreAutosave {
    pub positions: Vec<review_explore::EvidencePosition>,
    pub focus: Option<crate::ReviewPane>,
}

#[derive(Clone, Debug)]
pub struct ExploreHistoryChanged(pub review_explore::ExploreHistory);
