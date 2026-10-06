//! The frames the reviewer paints, as Herdr reports them: each painted frame ends with a
//! [`FrameMarker`] title, which Herdr reports as a pane update once it has read the frame.
//! Waiting on these events, a vision session reads a screen only once the reviewer has reacted.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value, json};
use vision_signal::FrameMarker;

use herdr_client::client::{EventCanceller, HerdrClient};

/// What a pane of the session's workspace last reported.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PaneFrames {
    /// The latest frame marker, `None` before the first frame.
    pub(crate) latest: Option<FrameMarker>,
    /// Whether the pane's process exited, or the pane closed.
    pub(crate) exited: bool,
    /// Whether Herdr detects an agent in the pane.
    pub(crate) agent: bool,
}

/// Why a wait for a frame ended without one.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Stalled {
    /// The pane's process exited.
    Exited,
    /// Herdr ended the event stream.
    Ended(String),
    /// No such frame came within the guard; the latest one, if any.
    Guard(Option<FrameMarker>),
}

impl std::error::Error for Stalled {}

impl std::fmt::Display for Stalled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exited => write!(formatter, "the pane's process exited"),
            Self::Ended(reason) => write!(formatter, "Herdr's event stream ended: {reason}"),
            Self::Guard(None) => write!(formatter, "nothing came in time, and no frame"),
            Self::Guard(Some(marker)) => write!(
                formatter,
                "nothing came in time; the latest frame is {} (acknowledging request {})",
                marker.frame, marker.acknowledged
            ),
        }
    }
}

#[derive(Default)]
struct State {
    panes: HashMap<String, PaneFrames>,
    ended: Option<String>,
}

/// The frames of the panes of one Herdr workspace.
pub(crate) struct Frames {
    workspace: String,
    state: Mutex<State>,
    changed: Condvar,
}

impl Frames {
    pub(crate) fn new(workspace: String) -> Self {
        Self {
            workspace,
            state: Mutex::default(),
            changed: Condvar::new(),
        }
    }

    /// The subscriptions whose events [`Self::observe`] reads.
    pub(crate) fn subscriptions() -> [Value; 4] {
        [
            json!({"type": "pane.updated"}),
            json!({"type": "pane.exited"}),
            json!({"type": "pane.closed"}),
            json!({"type": "pane.agent_detected"}),
        ]
    }

    /// Take one event of Herdr's stream into account.
    pub(crate) fn observe(&self, event: &str, data: &Value) {
        let (pane, title, exited) = match event {
            "pane_updated" => (
                &data["pane"],
                data["pane"]["terminal_title"].as_str(),
                false,
            ),
            "pane_exited" | "pane_closed" => (data, None, true),
            "pane_agent_detected" => (data, None, false),
            _ => return,
        };
        let Some(pane_id) = pane["pane_id"].as_str() else {
            return;
        };
        // A closed pane's event may not name its workspace; a pane ID names it first.
        let workspace = pane["workspace_id"]
            .as_str()
            .or_else(|| pane_id.split(':').next());
        if workspace != Some(self.workspace.as_str()) {
            return;
        }
        let mut state = self.lock();
        let frames = state.panes.entry(pane_id.to_owned()).or_default();
        frames.exited |= exited;
        if event == "pane_agent_detected" {
            frames.agent = data["agent"].is_string() && data["released"] != true;
        }
        if let Some(marker) = title.and_then(FrameMarker::parse) {
            frames.latest = Some(marker);
        }
        self.changed.notify_all();
    }

    /// Herdr's stream ended, so no frame comes any more.
    pub(crate) fn end(&self, reason: String) {
        self.lock().ended.get_or_insert(reason);
        self.changed.notify_all();
    }

    pub(crate) fn pane(&self, pane_id: &str) -> PaneFrames {
        self.lock().panes.get(pane_id).copied().unwrap_or_default()
    }

    /// Wait for a frame of `pane_id` that satisfies `until`, the latest one first, for at most
    /// `guard`.
    pub(crate) fn wait(
        &self,
        pane_id: &str,
        guard: Duration,
        mut until: impl FnMut(&FrameMarker) -> bool,
    ) -> Result<FrameMarker, Stalled> {
        self.wait_pane(pane_id, guard, |frames| {
            frames.latest.filter(|marker| until(marker))
        })
    }

    /// Wait until Herdr detects an agent in `pane_id`, for at most `guard`.
    pub(crate) fn wait_agent(&self, pane_id: &str, guard: Duration) -> Result<(), Stalled> {
        self.wait_pane(pane_id, guard, |frames| frames.agent.then_some(()))
    }

    /// Wait until the process of `pane_id` exits, for at most `guard`.
    pub(crate) fn wait_exit(&self, pane_id: &str, guard: Duration) -> Result<(), Stalled> {
        self.wait_pane(pane_id, guard, |frames| frames.exited.then_some(()))
    }

    /// Wait until `until` finds what it looks for in the state of `pane_id`, for at most
    /// `guard`, waking on each event.
    fn wait_pane<T>(
        &self,
        pane_id: &str,
        guard: Duration,
        mut until: impl FnMut(&PaneFrames) -> Option<T>,
    ) -> Result<T, Stalled> {
        let deadline = Instant::now() + guard;
        let mut state = self.lock();
        loop {
            let frames = state.panes.get(pane_id).copied().unwrap_or_default();
            if let Some(found) = until(&frames) {
                return Ok(found);
            }
            if frames.exited {
                return Err(Stalled::Exited);
            }
            if let Some(reason) = &state.ended {
                return Err(Stalled::Ended(reason.clone()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Stalled::Guard(frames.latest));
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A thread that feeds Herdr's events to [`Frames`] until dropped.
pub(crate) struct FrameWatch {
    frames: Arc<Frames>,
    canceller: EventCanceller,
    thread: Option<JoinHandle<()>>,
}

impl FrameWatch {
    /// Subscribe before the workspace's panes start, so no frame goes unseen.
    pub(crate) fn start(herdr: &HerdrClient, workspace: String) -> Result<Self> {
        let frames = Arc::new(Frames::new(workspace));
        let canceller = EventCanceller::default();
        let mut events = herdr.subscribe(&Frames::subscriptions(), &canceller)?;
        let observer = Arc::clone(&frames);
        let thread = std::thread::spawn(move || {
            loop {
                match events.next_event() {
                    Ok(Some(event)) => observer.observe(&event.event, &event.data),
                    Ok(None) => return observer.end("the session stopped watching".into()),
                    Err(error) => return observer.end(error.to_string()),
                }
            }
        });
        Ok(Self {
            frames,
            canceller,
            thread: Some(thread),
        })
    }

    pub(crate) fn frames(&self) -> &Frames {
        &self.frames
    }
}

impl Drop for FrameWatch {
    fn drop(&mut self) {
        self.canceller.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
#[path = "frames.tests.rs"]
mod tests;
