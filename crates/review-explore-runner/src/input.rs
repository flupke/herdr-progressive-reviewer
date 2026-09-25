//! Human-readable wakeups; full questions and answers remain in reviewer history.
use review_explore::{ReviewerAnswer, TopicStatus, TurnRequest};
use std::{fmt, path::Path};

pub(super) struct TurnInput<'a> {
    pub(super) request: &'a TurnRequest,
    pub(super) access: &'a str,
    pub(super) repository_root: Option<&'a Path>,
}

impl fmt::Display for TurnInput<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let request = self.request;
        writeln!(output, "Explore review access: {}", self.access)?;
        writeln!(output, "Explore pass: {}", request.instance)?;
        writeln!(output, "Explore request: {}", request.request)?;
        writeln!(
            output,
            "Review unit: {}",
            request.checkpoint.review_unit.as_str()
        )?;
        writeln!(output, "Checkpoint: {}", request.checkpoint.checkpoint)?;
        if let Some(root) = self.repository_root {
            writeln!(output, "Repository root: {}", root.display())?;
        }
        if let Some(error) = &request.response_error {
            writeln!(output, "\nPrevious response error:\n{error}")?;
        }
        if let Some(answer) = &request.answer {
            Self::answer(output, answer)?;
        }
        Ok(())
    }
}

impl TurnInput<'_> {
    fn answer(output: &mut fmt::Formatter<'_>, answer: &ReviewerAnswer) -> fmt::Result {
        writeln!(output, "\nAnswer ID: {}", answer.id)?;
        if let Some(question) = &answer.question {
            writeln!(
                output,
                "Question: {} (version {})",
                question.id, question.version
            )?;
        } else {
            writeln!(output, "Reply to conclusion: {}", answer.in_reply_to)?;
        }
        if let Some(original) = &answer.corrects {
            writeln!(output, "Corrects answer: {original}")?;
        }
        if answer.deferred {
            writeln!(output, "Explicitly deferred")?;
        }
        if let Some(option) = &answer.option {
            let outcome = match option.outcome {
                TopicStatus::Open => "open",
                TopicStatus::Accepted => "accepted",
                TopicStatus::NeedsFollowUp => "needs_follow_up",
                TopicStatus::Deferred => "deferred",
            };
            writeln!(
                output,
                "Selected option ID: {}\nSelected outcome: {outcome}",
                option.id
            )?;
            writeln!(output, "\nSelected option:\n{}", option.text)?;
        }
        if !answer.text.is_empty() {
            writeln!(output, "\nComment:\n{}", answer.text)?;
        }
        Ok(())
    }
}
