//! Start an Explore interview once, then wake the same agent for each human answer. Each turn's
//! instructions are files the review tool writes for the agent to read, trimmed to the round;
//! the prompt only names the round, where its index is, and the tools that end the turn.
use std::path::Path;

use review_explore::{ChangeAuthor, Comparison, TurnRequest};

mod decisions;
mod input;
mod rules;
mod unreviewed;

pub use decisions::EarlierDecisions;
pub use rules::TurnFile;
pub use unreviewed::Unreviewed;

/// What a round started with, the same on every turn: the decisions of the review's earlier
/// rounds and the reviewer's notes.
#[derive(Debug, Default)]
pub struct RoundData {
    pub earlier: EarlierDecisions,
    pub notes: Option<String>,
}

/// The index every turn's directory holds, which the prompt names.
pub const INDEX: &str = "index.md";

/// One agent turn's instructions, as the files of its directory.
#[derive(Clone, Debug)]
pub struct TurnInstructions {
    instance: String,
    files: Vec<TurnFile>,
}

impl TurnInstructions {
    /// The instructions for `request`, listing the unreviewed lines, with the round's data: the
    /// change, and what the `round` started with. `answered` is the number the reviewer's
    /// screens give the question the request's answer answers.
    pub fn prepare(
        request: &TurnRequest,
        comparison: &Comparison,
        access: &str,
        unreviewed: &Unreviewed,
        round: &RoundData,
        answered: Option<review_types::QuestionNumber>,
    ) -> Self {
        let round = input::Round {
            repository_root: &comparison.repository_root,
            description: comparison.change_description(),
            decisions: &round.earlier,
            author: request.author,
            reviewer: request.reviewer.as_deref(),
            notes: round.notes.as_deref(),
        };
        let input = input::TurnInput {
            request,
            access,
            unreviewed,
            answered,
        };
        Self {
            instance: request.instance.clone(),
            files: rules::files(request, comparison, &input.to_string(), &round.to_string()),
        }
    }

    /// The files to write in the turn's directory, the index first.
    pub fn files(&self) -> &[TurnFile] {
        &self.files
    }

    /// The prompt of the turn whose files are written in `directory`.
    pub fn prompt(&self, directory: &Path) -> String {
        format!(
            "Explore round {}: read {} and follow it. The turn ends in one call to the reviewer's \
             MCP tool submit_question or submit_conclusion.\n",
            self.instance,
            directory.join(INDEX).display()
        )
    }
}

/// The human's edited task list is the entire implementation scope. For someone else's change,
/// the list is the review to post, and posting it the entire scope.
pub fn implementation_prompt(
    request: &review_explore::ImplementationRequest,
    author: ChangeAuthor,
) -> String {
    if author == ChangeAuthor::Other {
        return format!(
            "The reviewer clicked Post review on the Explore conclusion of someone else's change. Post the review below on the change's pull request, with the repository's usual tool for it, such as gh: each comment on the file and line it names, the others in the review's body. This action authorizes posting these comments only: do not edit code, push, approve or request changes. If you cannot find the change's pull request, say so and stop.\n\nReview from the reviewer:\n{}",
            request.text
        );
    }
    format!(
        "The reviewer clicked Implement on the Explore conclusion. Implement the edited task list below in this conversation, following the repository instructions and validating the changes. This action authorizes code edits and tests for these tasks. Summary and future-work items are not additional implementation scope. Do not submit another review question or mark files reviewed as part of this action.\n\nTasks from the reviewer:\n{}",
        request.text
    )
}

#[cfg(test)]
mod tests;
