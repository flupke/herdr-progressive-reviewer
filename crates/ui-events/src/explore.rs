use review_explore::{Comparison, EvidenceRef, InterviewUpdate};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ExploreCaptured {
    pub result: Result<Arc<Comparison>, String>,
}

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
    pub result: Result<(), String>,
}

#[derive(Clone, Debug)]
pub struct ExploreEvidence {
    pub comparison: Arc<Comparison>,
    pub evidence: Vec<EvidenceRef>,
    pub primary: usize,
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

#[derive(Clone, Copy, Debug)]
pub struct ExploreFocusCycle {
    pub from_evidence: bool,
}
