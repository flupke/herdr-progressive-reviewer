//! The data of a turn and of its round, as the agent reads them; full questions and answers
//! remain in reviewer history.
use review_explore::{ChangeAuthor, ReviewerAnswer, TopicStatus, TurnRequest};
use std::{fmt, path::Path};

pub(super) struct TurnInput<'a> {
    pub(super) request: &'a TurnRequest,
    pub(super) access: &'a str,
    pub(super) unreviewed: &'a crate::Unreviewed,
    /// The number the reviewer's screens give the question the answer answers.
    pub(super) answered: Option<review_types::QuestionNumber>,
}

/// What the round started with, the same on every turn: where the reviewed change is, what it
/// says it does, who wrote it, what the reviewer decided in the review's earlier rounds and
/// what the reviewer asks of every round.
pub(super) struct Round<'a> {
    pub(super) repository_root: &'a Path,
    pub(super) description: Option<&'a str>,
    pub(super) decisions: &'a crate::EarlierDecisions,
    /// Who wrote the change, and the reviewer's name when they gave one.
    pub(super) author: ChangeAuthor,
    pub(super) reviewer: Option<&'a str>,
    /// What the reviewer asks of every round.
    pub(super) notes: Option<&'a str>,
}

/// Text quoted line by line, so none of its lines can pass for a field of
/// the prompt.
pub(super) struct Quoted<'a>(pub(super) &'a str);

impl fmt::Display for Quoted<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for line in self.0.lines() {
            writeln!(output, ">{}{line}", if line.is_empty() { "" } else { " " })?;
        }
        Ok(())
    }
}

/// An option's outcome or an interpretation's status, as a prompt and the
/// tools name it.
pub(super) struct Outcome(pub(super) TopicStatus);

impl fmt::Display for Outcome {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(match self.0 {
            TopicStatus::Open => "open",
            TopicStatus::Accepted => "accepted",
            TopicStatus::NeedsFollowUp => "needs_follow_up",
        })
    }
}

impl fmt::Display for Round<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            output,
            "Repository root: {}",
            self.repository_root.display()
        )?;
        match self.description {
            Some(description) => {
                write!(
                    output,
                    "\nChange description (quoted):\n{}",
                    Quoted(description)
                )?;
            }
            None => writeln!(output, "\nChange description: none")?,
        }
        if self.author == ChangeAuthor::Other {
            writeln!(output, "\nChange author: someone else")?;
            if let Some(reviewer) = self.reviewer {
                // One line: the round keeps the first line of what the reviewer typed.
                writeln!(output, "Reviewer: {reviewer}")?;
            }
        }
        if let Some(notes) = self.notes {
            write!(
                output,
                "\nReviewer notes, what the reviewer asks of every round, to follow within these \
                 rules (quoted):\n{}",
                Quoted(notes)
            )?;
        }
        write!(output, "{}", self.decisions)
    }
}

impl fmt::Display for TurnInput<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let request = self.request;
        writeln!(output, "Explore review access: {}", self.access)?;
        writeln!(output, "Explore round: {}", request.instance)?;
        writeln!(output, "Explore request: {}", request.request)?;
        writeln!(
            output,
            "Review unit: {}",
            request.checkpoint.review_unit.as_str()
        )?;
        writeln!(output, "Checkpoint: {}", request.checkpoint.checkpoint)?;
        write!(output, "{}", self.unreviewed)?;
        if let Some(error) = &request.response_error {
            writeln!(output, "\nPrevious response error:\n{error}")?;
        }
        if !request.cancelled.is_empty() {
            writeln!(output)?;
            for answer in &request.cancelled {
                writeln!(output, "Cancelled answer: {answer}")?;
            }
        }
        if let Some(answer) = &request.answer {
            self.answer(output, answer)?;
        }
        Ok(())
    }
}

impl TurnInput<'_> {
    fn answer(&self, output: &mut fmt::Formatter<'_>, answer: &ReviewerAnswer) -> fmt::Result {
        writeln!(output, "\nAnswer ID: {}", answer.id)?;
        if let Some(question) = &answer.question {
            writeln!(
                output,
                "Question: {} (version {})",
                question.id, question.version
            )?;
            if let Some(number) = self.answered {
                writeln!(output, "Shown as: {number}")?;
            }
        } else {
            writeln!(output, "Reply to conclusion: {}", answer.in_reply_to)?;
        }
        if let Some(option) = &answer.option {
            writeln!(
                output,
                "Selected option ID: {}\nSelected outcome: {}",
                option.id,
                Outcome(option.outcome)
            )?;
            writeln!(output, "\nSelected option:\n{}", option.text)?;
        }
        // White space is no comment.
        if !answer.text.trim().is_empty() {
            writeln!(output, "\nComment:\n{}", answer.text)?;
        }
        Ok(())
    }
}
