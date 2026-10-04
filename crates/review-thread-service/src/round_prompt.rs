//! What a wakeup tells the agent about the round conversations among the pending threads.

use std::fmt;

use review_threads::{AskedUnder, Author, ReviewThread};

/// The round conversations among the threads a wakeup brings, with each message the agent
/// has not answered yet and where in its round the reviewer wrote it.
pub(super) struct RoundConversations<'a>(pub(super) &'a [ReviewThread]);

impl RoundConversations<'_> {
    fn conversations(&self) -> impl Iterator<Item = (&ReviewThread, &str)> {
        self.0
            .iter()
            .filter_map(|thread| Some((thread, thread.round()?)))
    }
}

impl fmt::Display for RoundConversations<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.conversations().next().is_none() {
            return Ok(());
        }
        write!(
            output,
            "\n\nSome of these threads are the conversation of an Explore round: the reviewer talks with you beside the round's questions. Such a message does not answer the question it was asked under; that question stays open and its answer still comes through Explore. Read the round's conversation with get_new_messages and answer each message with the reply tool only: do not call submit_question or submit_conclusion for it, and do not edit code. Each message names the question it was asked under, or the stage of the round, and may quote a passage of the round."
        )?;
        for (thread, round) in self.conversations() {
            write!(
                output,
                "\n\nRound conversation: {}\nExplore round: {round}\n",
                thread.id.as_str()
            )?;
            for message in thread.messages.iter().filter(|message| {
                message.author == Author::Reviewer && !thread.is_answered(message)
            }) {
                writeln!(output, "Message: {}", message.id.as_str())?;
                match &message.asked_under {
                    Some(AskedUnder::Question {
                        question,
                        version,
                        number,
                    }) => {
                        writeln!(output, "Question: {question} (version {version})")?;
                        if let Some(number) = number {
                            writeln!(output, "Shown as: {number}")?;
                        }
                    }
                    Some(AskedUnder::Design) => writeln!(output, "Stage: design")?,
                    Some(AskedUnder::Conclusion { conclusion }) => {
                        writeln!(output, "Stage: conclusion\nConclusion: {conclusion}")?;
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }
}
