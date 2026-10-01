//! Durable interviews over the complete working-copy change, retaining compact diff patches.

mod coverage;
mod durable;
mod exclusion;
mod presentation;
mod recovery;
pub use coverage::{
    ChangedLineCoverage, ClassificationProgress, ClassificationState, CoverageFeedback,
    CoverageInventory, CoverageLedger, CoverageReceipt, CoverageSummary, CoverageUnit,
    ExcludedLines, FileCoverage, Gap, GapKind, GapPage, GapQuery, JevFeedback, JevMode,
    Significance, SignificanceResult, UncoveredArea, UncoveredOverview,
};
pub use durable::{
    ConversationBinding, DispatchId, DispatchResult, DispatchState, ExploreHistory, ExplorePass,
    ImplementationDelivery, InterviewDelivery, ReviewCompletion, UnexploredAtConclusion,
};
pub use exclusion::{ExclusionPolicy, SignificanceClassifier, SignificancePlan};
pub use presentation::{
    EditorFocus, EvidencePosition, ExploreDraft, ExplorePage, ExploreViewState, QuestionReading,
    ViewSave,
};

mod agenda;
mod agenda_validation;
mod capture;
mod choices;
mod conclusion;
mod consequence;
mod conversation;
mod inspection;
mod interview;
mod path_serde;
mod source;
mod validation;

pub use agenda::{AgendaAction, AgendaChange};
pub use capture::{Comparison, ManifestEntry};
pub use conclusion::{Conclusion, ConclusionSubmission, ImplementationRequest};
pub use consequence::{Assessments, Consequence, Door};
pub use conversation::{ConversationTurn, Reply};
pub use inspection::{Inspection, InspectionDisposition};
pub use interview::{
    Alternative, AnswerInput, Exploration, Interpretation, InterviewUpdate, Question,
    ReviewerAnswer, Topic, TopicStatus, TurnRequest,
};
pub use source::{CodeLocation, EvidenceRef, Source, SourceSide};

/// Work requested explicitly from Explore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Start,
    Retry(Box<TurnRequest>),
    SaveView(Box<ViewSave>),
    Turn(Box<TurnRequest>),
    Implement(ImplementationRequest),
    RequireReview(Box<Vec<CoverageUnit>>),
    CancelImplementation,
    Cancel,
}
