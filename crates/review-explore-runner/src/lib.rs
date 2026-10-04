//! Start an Explore interview once, then wake the same agent for each human answer.
use markdown_marks::{Callout, Mark, StatusMark};
use review_explore::{Comparison, TurnRequest};
use review_explore_round_settings::WritingStyle;

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
        let instructions =
            Self::instructions(kickoff.is_some(), request.challenger, request.writing);
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

    /// The rules of a turn: the kickoff's, or a later turn's, followed by the
    /// sections every prompt states (the conclusion's quiz, then Not relevant),
    /// by the challenger's script when the round has one, and by the rules of
    /// the round's writing style when it has some.
    fn instructions(kickoff: bool, challenger: bool, writing: WritingStyle) -> String {
        let turn = if kickoff {
            format!(
                "{}\n\n{}\n\n{}",
                include_str!("interview.md").trim_end(),
                include_str!("behavior_changes.md").trim_end(),
                Self::explanations()
            )
        } else {
            include_str!("wakeup.md").trim_end().to_owned()
        };
        let mut instructions = format!(
            "{}\n\n{}\n\n{}",
            turn,
            include_str!("quiz.md").trim_end(),
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
        if let Some(style) = Self::writing(kickoff, writing) {
            instructions.push_str("\n\n");
            instructions.push_str(style.trim_end());
        }
        instructions
    }

    /// The rules of the writing style `writing`: in full in the kickoff, and as a short
    /// reminder in every later turn, so that the style holds in a long round, whose earlier
    /// turns the agent may have compacted, and after a restore. The plain style has none.
    fn writing(kickoff: bool, writing: WritingStyle) -> Option<&'static str> {
        match (writing, kickoff) {
            (WritingStyle::Plain, _) => None,
            (WritingStyle::SimplifiedTechnicalEnglish, true) => Some(include_str!("writing.md")),
            (WritingStyle::SimplifiedTechnicalEnglish, false) => {
                Some(include_str!("writing_wakeup.md"))
            }
        }
    }

    /// How to write the Markdown of an explanation, with the markers of its callouts and status
    /// marks, and its diagrams.
    fn explanations() -> String {
        fn markers<M: Mark>() -> String {
            M::ALL
                .iter()
                .map(|mark| format!("`{}`", mark.marker()))
                .collect::<Vec<_>>()
                .join(", ")
        }
        format!(
            "{}\n\nA callout is a quote that opens with its marker, as in `> {} Run it twice.`: {}.\n\
             A status mark opens a table cell, as in `| {} 2 ms |`: {}.\n\n{}",
            include_str!("explanation.md").trim_end(),
            Callout::Tip.marker(),
            markers::<Callout>(),
            StatusMark::Good.marker(),
            markers::<StatusMark>(),
            Self::diagrams(),
        )
    }

    /// When an explanation draws a diagram, how the page draws it, and how to avoid parse
    /// errors.
    fn diagrams() -> String {
        format!(
            "A diagram is a fenced block that opens with ```` ```{} ````, which the Explore page \
             draws with Mermaid {}, where the reviewer follows the round on that page.\n{}",
            mermaid_js::FENCE,
            mermaid_js::VERSION,
            include_str!("diagrams.md").trim_end(),
        )
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
