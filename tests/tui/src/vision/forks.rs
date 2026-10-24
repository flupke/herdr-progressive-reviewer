//! What the stand-in agent and its forks reported on the session's event socket, as the driver
//! reads it: each fork run-ahead started, with its prompt and how far it got, and the sessions the
//! agent ran.

use review_test_support::stand_in::{Reported, StandInEvent, StandInRole};
use serde::Serialize;

/// One fork, in the order the forks started.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Fork {
    /// Its session, which the driver names to play it.
    pub(crate) session: String,
    /// The session it copies.
    parent: String,
    /// The choice it takes the turn of, from its prompt.
    choice: Option<String>,
    /// The prompt it read, the turn's files appended.
    pub(crate) prompt: String,
    /// Its output says that its submit has its answer.
    submitted: bool,
    /// It ended its turn by itself.
    finished: bool,
    /// Its process ended, however it ended: the reviewer stops a fork it discards.
    pub(crate) exited: bool,
}

/// The sessions of the agent in the pane.
#[derive(Debug, Default, Eq, PartialEq, Serialize)]
pub(crate) struct AgentSessions {
    /// The session it reported last, to Herdr and the reviewer's hooks.
    pub(crate) session: Option<String>,
    /// The sessions it resumed with Claude Code's `/resume`, in order.
    resumed: Vec<String>,
}

/// The forks among the events `received`, in the order they started.
pub(crate) fn forks(received: &[Reported]) -> Vec<Fork> {
    let mut forks: Vec<Fork> = Vec::new();
    for reported in received {
        let StandInRole::Fork { session } = &reported.from else {
            continue;
        };
        if let StandInEvent::ForkStarted { parent, prompt } = &reported.event {
            forks.push(Fork {
                session: session.clone(),
                parent: parent.clone(),
                choice: prompt_line(prompt, "Selected option ID: "),
                prompt: prompt.clone(),
                submitted: false,
                finished: false,
                exited: false,
            });
            continue;
        }
        let Some(fork) = forks.iter_mut().find(|fork| fork.session == *session) else {
            continue;
        };
        match reported.event {
            StandInEvent::ForkSubmitted => fork.submitted = true,
            StandInEvent::ForkFinished => fork.finished = true,
            StandInEvent::Disconnected => fork.exited = true,
            _ => {}
        }
    }
    forks
}

/// The sessions of the agent among the events `received`.
pub(crate) fn agent_sessions(received: &[Reported]) -> AgentSessions {
    let mut sessions = AgentSessions::default();
    for reported in received
        .iter()
        .filter(|reported| reported.from == StandInRole::Agent)
    {
        match &reported.event {
            StandInEvent::SessionReported { session, .. } => {
                sessions.session = Some(session.clone());
            }
            StandInEvent::ResumeReceived { session } => sessions.resumed.push(session.clone()),
            _ => {}
        }
    }
    sessions
}

/// The rest of the first line of `prompt` that starts with `label`.
pub(crate) fn prompt_line(prompt: &str, label: &str) -> Option<String> {
    prompt
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .map(str::to_owned)
}

#[cfg(test)]
#[path = "forks.tests.rs"]
mod tests;
