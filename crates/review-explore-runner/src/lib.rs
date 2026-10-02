//! Start an Explore interview once, then wake the same agent for each human answer.
use review_explore::{Comparison, TurnRequest};

mod input;
mod unreviewed;

pub use unreviewed::Unreviewed;

#[derive(Debug)]
pub struct PreparedTurn {
    prompt: String,
}

impl PreparedTurn {
    /// The prompt for `request`, listing the unreviewed lines.
    pub fn prepare(
        request: &TurnRequest,
        comparison: &Comparison,
        access: &str,
        unreviewed: &Unreviewed,
    ) -> Self {
        let kickoff = request.answer.is_none().then(|| input::Kickoff {
            repository_root: &comparison.repository_root,
            description: comparison.change_description(),
        });
        let instructions = if kickoff.is_some() {
            include_str!("interview.md")
        } else {
            include_str!("wakeup.md")
        };
        let input = input::TurnInput {
            request,
            access,
            kickoff,
            unreviewed,
        };
        Self {
            prompt: format!("{}\n\n{input}", instructions.trim_end()),
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
