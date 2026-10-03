//! Durable interviews over the complete working-copy change, retaining compact diff patches.

mod durable;
mod presentation;
mod recovery;
pub use durable::{
    ConversationBinding, DispatchId, DispatchResult, DispatchState, ExploreHistory, ExploreRound,
    ImplementationDelivery, InterviewDelivery, MarkCounts, ReopenedLines, ReviewCompletion,
    TurnMarks,
};
pub use presentation::{
    EditorFocus, EvidencePosition, ExplorePage, ExploreViewState, QuestionReading, ViewSave,
};

mod agenda;
mod agenda_validation;
mod cancel;
mod capture;
mod challenger;
mod choices;
mod conclusion;
mod consequence;
mod conversation;
mod interview;
mod not_relevant;
mod path_serde;
mod sections;
mod source;
mod validation;

pub use agenda::{AgendaAction, AgendaChange};
pub use cancel::CancelledAnswer;
pub use capture::{Comparison, ManifestEntry};
pub use challenger::{ChallengerProposal, ProposalResult};
pub use conclusion::{Conclusion, ConclusionSubmission, ImplementationRequest};
pub use consequence::{Assessments, Consequence, Door};
pub use conversation::{ConversationTurn, Reply};
pub use interview::{
    Alternative, AnswerInput, Exploration, Interpretation, InterviewUpdate, Question,
    ReviewerAnswer, Topic, TopicStatus, TurnRequest,
};
pub use not_relevant::{NotRelevantMark, NotRelevantReason, TestLocation};
pub use sections::QuestionSection;
pub use source::{CodeLocation, EvidenceRef, Source, SourceSide};

/// Work requested explicitly from Explore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Start,
    /// Close the round and return to the start screen.
    Reset,
    Retry(Box<TurnRequest>),
    SaveView(Box<ViewSave>),
    Turn(Box<TurnRequest>),
    Implement(ImplementationRequest),
    CancelImplementation,
    Cancel,
    /// Cancel the reviewer's latest answer, by ID.
    CancelAnswer(String),
}
