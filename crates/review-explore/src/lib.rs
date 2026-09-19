//! Durable interviews over the complete working-copy change, without source archives.

mod durable;
mod presentation;
mod recovery;
pub use durable::{
    ConversationBinding, DispatchId, DispatchResult, DispatchState, ExplorePass,
    ImplementationDelivery, InterviewDelivery,
};
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
mod interview;
mod path_serde;
mod source;
mod validation;

pub use agenda::{AgendaAction, AgendaChange};
pub use capture::{Comparison, ManifestEntry};
pub use conclusion::{Conclusion, ConclusionSubmission, ImplementationRequest};
pub use consequence::{Assessments, Consequence, Door};
pub use conversation::{ConversationTurn, Reply};
pub use interview::{
    Alternative, AnswerInput, Exploration, Interpretation, InterviewUpdate, Question,
    ReviewerAnswer, Topic, TopicStatus, TurnRequest,
};
pub use source::{CodeLocation, EvidenceRef, Source, SourceSide};

/// Work requested explicitly from Explore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Start,
    SaveView(Box<ViewSave>),
    OpenPass(String),
    Turn(Box<TurnRequest>),
    Implement(ImplementationRequest),
    CancelImplementation,
    Cancel,
}
