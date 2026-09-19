//! Start an Explore interview once, then wake the same agent for each human answer.
use review_explore::{Comparison, TurnRequest};

mod input;

#[derive(Debug)]
pub struct PreparedTurn {
    prompt: String,
}

impl PreparedTurn {
    pub fn prepare(request: &TurnRequest, comparison: &Comparison, access: &str) -> Self {
        let instructions = if request.answer.is_some() {
            "Continue Explore with this answer; call submit_question or, when finished, submit_conclusion. Match the question ID/version to your earlier question. Keep review-only scope. Repair any previous response error without changing the answer or decisions."
        } else {
            include_str!("interview.md")
        };
        let input = input::TurnInput {
            request,
            access,
            repository_root: request
                .answer
                .is_none()
                .then_some(comparison.repository_root.as_path()),
        };
        Self {
            prompt: format!("{instructions}\n\n{input}"),
        }
    }

    pub fn prompt(self) -> String {
        self.prompt
    }
}

/// The human's edited task list is the entire implementation scope.
pub fn implementation_prompt(request: &review_explore::ImplementationRequest) -> String {
    format!(
        "The reviewer clicked Implement on the Explore conclusion. Implement the edited task list below in this conversation, following the repository instructions and validating the changes. This action authorizes code edits and tests for these tasks. Summary and future-work items are not additional implementation scope. Do not submit another review question or mark files reviewed as part of this action.\n\nTasks from the reviewer:\n{}",
        request.text
    )
}

#[cfg(test)]
mod tests;
