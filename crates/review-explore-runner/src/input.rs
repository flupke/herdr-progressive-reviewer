//! Human-readable wakeups; full questions and answers remain in reviewer history.
use review_explore::{CoverageFeedback, ReviewerAnswer, TopicStatus, TurnRequest};
use std::{fmt, path::Path};

pub(super) struct TurnInput<'a> {
    pub(super) request: &'a TurnRequest,
    pub(super) access: &'a str,
    pub(super) repository_root: Option<&'a Path>,
    pub(super) feedback: Option<&'a CoverageFeedback>,
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
        if request.answer.is_some()
            && let Some(feedback) = self.feedback
        {
            Self::coverage(output, feedback)?;
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
    fn coverage(output: &mut fmt::Formatter<'_>, feedback: &CoverageFeedback) -> fmt::Result {
        let coverage = feedback
            .covered_percent_tenths
            .map_or("unavailable".into(), |value| {
                format!("{}.{:01}%", value / 10, value % 10)
            });
        let mode = match feedback.jev.mode {
            review_explore::JevMode::Enabled => "enabled",
            review_explore::JevMode::Disabled => "disabled",
        };
        writeln!(
            output,
            "\nCurrent answered-evidence coverage: {coverage}; {} required change units remain across {} files (revision {}, Jev {mode}).",
            feedback.summary.remaining, feedback.uncovered.total_files, feedback.revision,
        )?;
        if !feedback.uncovered.directories.is_empty() {
            write!(output, "Uncovered directory groups:")?;
            for area in feedback.uncovered.directories.iter().take(5) {
                write!(output, " {} {}/{};", area.path, area.remaining, area.files)?;
            }
            writeln!(output)?;
        }
        if !feedback.uncovered.files.is_empty() {
            write!(output, "Uncovered files:")?;
            for area in feedback.uncovered.files.iter().take(8) {
                write!(output, " {} {};", area.path, area.remaining)?;
            }
            writeln!(output)?;
        }
        writeln!(
            output,
            "Use get_coverage_gaps with this pass, checkpoint, revision and Jev mode to page or filter remaining regions."
        )
    }

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
