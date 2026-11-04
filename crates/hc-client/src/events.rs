//! herdr-core's events, and the agents' arrivals and departures the reviewer follows in them.

use std::collections::HashMap;

use herdr_client::protocol::{AgentStatus, HerdrEvent, PaneId, TabId, WorkspaceId};
use herdr_client::{Error, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::terminal::{Status, Terminal, place_of};

/// One fact change, as an `event` notification of `events.subscribe` carries it: of a terminal,
/// or of a document, such as herdr-app's layout, which names no terminal.
#[derive(Debug, Deserialize)]
pub(crate) struct Event {
    /// `created`, `restarted`, `status`, `labels`, `finished`, `removed`, `lost`, `turn`,
    /// `foreground`, `doc`...
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(default)]
    terminal: Option<String>,
    #[serde(default)]
    data: Value,
}

/// The data of a `status` event: the terminal's new OSC 7501 report, `None` once its program
/// exited.
#[derive(Debug, Deserialize)]
pub(crate) struct StatusChange {
    #[serde(default)]
    status: Option<Status>,
}

/// The data of a `labels` event: the terminal's labels now, and the whole terminal when it
/// enters a subscription by its labels.
#[derive(Debug, Deserialize)]
struct LabelsChange {
    #[serde(default)]
    labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    record: Option<Terminal>,
}

impl Event {
    /// The event's data as a `T`.
    pub(crate) fn data<T: DeserializeOwned>(&self) -> Result<T> {
        T::deserialize(&self.data).map_err(|source| Error::Json {
            operation: "read a herdr-core event",
            source,
        })
    }

    /// Whether the terminal's program is gone for good: it finished, it was removed, or it was
    /// lost, after which herdr-core gives the terminal a new shell at most.
    pub(crate) fn ends_program(&self) -> bool {
        matches!(self.kind.as_str(), "finished" | "removed" | "lost")
    }
}

/// What the reviewer knows of each terminal of a subscription, to tell an agent that appears in
/// a terminal, or leaves it, as Herdr's `pane.agent_detected` tells. A terminal holds an agent
/// while its program reports a status and it is in a tab of herdr-app.
#[derive(Debug, Default)]
pub(crate) struct Roster {
    terminals: HashMap<String, Known>,
}

#[derive(Clone, Debug, Default)]
struct Known {
    /// The space and the tab of herdr-app the terminal shows in.
    place: Option<(WorkspaceId, TabId)>,
    status: Option<Status>,
}

/// The agent a terminal holds: where it shows, and its status.
type Held<'a> = (&'a (WorkspaceId, TabId), &'a Status);

impl Known {
    fn of(terminal: Terminal) -> Self {
        Self {
            place: terminal.place(),
            status: terminal.status,
        }
    }

    fn agent(&self) -> Option<Held<'_>> {
        self.place.as_ref().zip(self.status.as_ref())
    }
}

impl Roster {
    /// The roster of the terminals of a subscription's snapshot: the agents there are known,
    /// so none of them is told.
    pub(crate) fn new(snapshot: Vec<Terminal>) -> Self {
        Self {
            terminals: snapshot
                .into_iter()
                .map(|terminal| (terminal.address.clone(), Known::of(terminal)))
                .collect(),
        }
    }

    /// The arrival or departure of an agent that `event` makes, if any.
    pub(crate) fn observe(&mut self, event: &Event) -> Result<Option<HerdrEvent>> {
        let Some(terminal) = &event.terminal else {
            return Ok(None);
        };
        let after = match event.kind.as_str() {
            // A lost terminal comes back as `restarted`, with its whole record.
            "created" | "restarted" => Known::of(event.data()?),
            "status" => Known {
                status: event.data::<StatusChange>()?.status,
                ..self.known(terminal)
            },
            "labels" => {
                let change: LabelsChange = event.data()?;
                match change.record {
                    Some(record) if !self.terminals.contains_key(terminal) => Known::of(record),
                    _ => Known {
                        place: place_of(&change.labels),
                        ..self.known(terminal)
                    },
                }
            }
            _ if event.ends_program() => Known::default(),
            _ => return Ok(None),
        };
        let before = self.terminals.insert(terminal.clone(), after.clone());
        if event.kind == "removed" {
            self.terminals.remove(terminal);
        }
        Ok(change(
            terminal,
            before.as_ref().and_then(Known::agent),
            after.agent(),
        ))
    }

    fn known(&self, terminal: &str) -> Known {
        self.terminals.get(terminal).cloned().unwrap_or_default()
    }
}

/// What a terminal's agent going from `before` to `after` tells: an arrival when an agent
/// appears, another replaces it, or it moves to another tab, which the reviewers of both tabs
/// look at again; a departure when it goes.
fn change(terminal: &str, before: Option<Held<'_>>, after: Option<Held<'_>>) -> Option<HerdrEvent> {
    let pane_id = PaneId(terminal.to_owned());
    match (before, after) {
        (Some(((workspace, _), was)), None) => Some(HerdrEvent::AgentDetected {
            pane_id,
            workspace_id: workspace.clone(),
            agent: was.agent(),
            released: true,
            final_status: Some(was.agent_status()),
        }),
        (Some((was_at, was)), Some((at, now))) if was_at == at && was.agent() == now.agent() => {
            None
        }
        (_, Some(((workspace, _), now))) => Some(HerdrEvent::AgentDetected {
            pane_id,
            workspace_id: workspace.clone(),
            agent: now.agent(),
            released: false,
            final_status: None,
        }),
        (None, None) => None,
    }
}

/// The status an agent's `status` event gives: unknown once its program stopped reporting.
pub(crate) fn status_of(change: &StatusChange) -> AgentStatus {
    change
        .status
        .as_ref()
        .map_or(AgentStatus::Unknown, Status::agent_status)
}

#[cfg(test)]
#[path = "events.tests.rs"]
mod tests;
