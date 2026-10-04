//! Durable interviews over the complete working-copy change, retaining compact diff patches.

mod durable;
mod presentation;
mod recovery;
pub use durable::{
    ConversationBinding, DispatchId, DispatchResult, DispatchState, ExploreHistory, ExploreRound,
    ImplementationDelivery, InterviewDelivery, MarkCounts, MarkTense, ReopenedLines,
    ReviewCompletion, TurnMarks,
};
pub use presentation::{
    EditorFocus, EvidencePosition, ExplorePage, ExploreViewState, QuestionReading, RoundFront,
    ViewSave,
};

mod agenda;
mod agenda_validation;
mod cancel;
mod capture;
mod challenger;
mod choices;
mod citation;
mod conclusion;
mod consequence;
mod conversation;
mod design;
mod diagram;
mod interview;
mod named_lines;
mod not_relevant;
mod overview;
mod path_serde;
mod quiz;
mod sections;
mod source;
mod start;
mod validation;

pub use agenda::{AgendaAction, AgendaChange};
pub use cancel::CancelledAnswer;
pub use capture::{Comparison, ManifestEntry};
pub use challenger::{ChallengerProposal, ProposalResult};
pub use citation::{CitedLines, CitedSource, Uncitable};
pub use conclusion::{Conclusion, ConclusionSubmission, ImplementationRequest};
pub use consequence::{Assessments, Consequence, Door};
pub use conversation::{ConversationTurn, Reply};
pub use design::{Design, DesignPart, DesignSection};
pub use diagram::DiagramError;
pub use interview::{
    Alternative, AnswerInput, Exploration, Interpretation, InterviewUpdate, Question,
    ReviewerAnswer, Topic, TopicStatus, TurnRequest,
};
pub use named_lines::NamedLines;
pub use not_relevant::{NotRelevantMark, NotRelevantReason, TestLocation};
pub use overview::{
    Activity, AgentRecord, Decision, DecisionTag, EarlierQuestion, KeptAnswer, LatestTurn,
    QuizProgress, QuizStage, RailStep, RoundOverview, RoundStanding, Step, StepState, TabTitle,
};
pub use quiz::{QuizAnswers, QuizItem, QuizPick, QuizResponse};
pub use sections::QuestionSection;
pub use source::{CodeLocation, EvidenceRef, Source, SourceSide};
pub use start::StartBlock;

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
