//! Start an Explore interview once, then wake the same agent for each human answer.
use review_explore::{Comparison, TurnRequest};

mod decisions;
mod input;
mod unreviewed;

pub use decisions::EarlierDecisions;
pub use unreviewed::Unreviewed;

#[derive(Debug)]
pub struct PreparedTurn {
    prompt: String,
}

impl PreparedTurn {
    /// The prompt for `request`, listing the unreviewed lines. A kickoff
    /// also lists the `earlier` decisions of the review.
    pub fn prepare(
        request: &TurnRequest,
        comparison: &Comparison,
        access: &str,
        unreviewed: &Unreviewed,
        earlier: &EarlierDecisions,
    ) -> Self {
        let kickoff = request.is_kickoff().then(|| input::Kickoff {
            repository_root: &comparison.repository_root,
            description: comparison.change_description(),
            decisions: earlier,
        });
        let instructions = Self::instructions(kickoff.is_some(), request.challenger);
        let input = input::TurnInput {
            request,
            access,
            kickoff,
            unreviewed,
        };
        Self {
            prompt: format!("{instructions}\n\n{input}"),
        }
    }

    /// The rules of a turn: the kickoff's, or a later turn's. Both end with
    /// the same Not relevant section, so every prompt states its criteria,
    /// then with the challenger's script when the round has one.
    fn instructions(kickoff: bool, challenger: bool) -> String {
        let turn = if kickoff {
            include_str!("interview.md")
        } else {
            include_str!("wakeup.md")
        };
        let mut instructions = format!(
            "{}\n\n{}",
            turn.trim_end(),
            include_str!("not_relevant.md").trim_end()
        );
        if challenger {
            let script = if kickoff {
                include_str!("challenger.md")
            } else {
                include_str!("challenger_wakeup.md")
            };
            instructions.push_str("\n\n");
            instructions.push_str(script.trim_end());
        }
        instructions
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
