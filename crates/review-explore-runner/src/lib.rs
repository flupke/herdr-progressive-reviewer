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
            "Continue Explore with this answer; match its question ID/version. Keep review-only scope and preserve the answer and decisions when repairing errors. Use submit_question for useful concept inquiries. Once those are exhausted, inspect remaining coverage gaps for missed questions before submit_conclusion. Coverage alone never ends the interview."
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

    #[must_use]
    pub fn with_coverage(mut self, feedback: &review_explore::CoverageFeedback) -> Self {
        use std::fmt::Write;
        let _ = write!(
            self.prompt,
            "\n\nCoverage inspection reminder, revision {}: {} non-exempt change units remain outside answered evidence; {} regions in this bounded listing{}. This is not an interview stopping condition.",
            feedback.revision,
            feedback.summary.remaining,
            feedback.total_gaps,
            if feedback.has_more {
                " (more remain)"
            } else {
                ""
            }
        );
        for gap in feedback.unassigned_required.iter().take(8) {
            let _ = write!(
                self.prompt,
                "\nUnassigned: {} {:?} {:?}",
                gap.location.path.display(),
                gap.location.side,
                gap.location.lines
            );
        }
        for gap in feedback.awaiting_answer.iter().take(8) {
            let _ = write!(
                self.prompt,
                "\nAwaiting answer: {} {:?} {:?}",
                gap.location.path.display(),
                gap.location.side,
                gap.location.lines
            );
        }
        self
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
