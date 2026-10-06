//! Decide, once per event, which part of the runtime handles it.

use std::time::Instant;

use component_core::EventEnvelope;
use herdr_client::protocol::{HerdrEvent, PaneId};
use review_ui::UserInput;

/// A background worker thread ended while the runtime still needed it.
pub(super) struct WorkerStopped;
/// A stop signal asked the runtime to quit.
pub(super) struct StopRequested;
/// The repository watcher saw changes worth a refresh.
pub(super) struct RepositoryRefreshDue;
/// The periodic timer fired.
pub(super) struct ApplicationTick(pub(super) Instant);
/// The terminal could no longer be read.
pub(super) struct TerminalFailed(pub(super) String);
/// The terminal regained focus.
pub(super) struct TerminalFocused;
/// The driver of a vision session asked for this acknowledgement after its input.
pub(super) struct InputAcknowledged(pub(super) u64);

/// Where one event goes.
///
/// This is the only place that inspects event types; the event loop acts on the
/// route and never downcasts an event itself.
#[derive(Debug)]
pub(super) enum Route<'a> {
    /// The runtime cannot continue.
    Fail(String),
    /// The runtime stops cleanly.
    Stop,
    /// The user focused another Herdr pane, which may be the agent to prompt.
    AgentFocused(&'a PaneId),
    /// Other Herdr traffic belongs to agent delivery, not to the application.
    Herdr(&'a HerdrEvent),
    /// The repository needs a refresh, which the application shows as started.
    RefreshDue,
    /// The periodic timer, which drives animations and toast expiry.
    Tick(Instant),
    /// Terminal input for the application.
    Input(&'a UserInput),
    /// A language-server event for the application.
    Lsp(&'a review_lsp::Event),
    /// The terminal regained focus; the application sees the event as-is.
    TerminalFocused,
    /// The next frame marker names this acknowledgement, after the input that came before it.
    Acknowledged(u64),
    /// Any other event, published to the application unchanged.
    Application,
}

impl<'a> Route<'a> {
    pub(super) fn of(event: &'a EventEnvelope) -> Self {
        if let Some(event) = event.downcast_ref::<HerdrEvent>() {
            return match event {
                HerdrEvent::PaneFocused(pane_id) => Self::AgentFocused(pane_id),
                event @ HerdrEvent::AgentDetected { .. } => Self::Herdr(event),
            };
        }
        if let Some(input) = event.downcast_ref::<UserInput>() {
            return Self::Input(input);
        }
        if let Some(ApplicationTick(now)) = event.downcast_ref::<ApplicationTick>() {
            return Self::Tick(*now);
        }
        if let Some(event) = event.downcast_ref::<review_lsp::Event>() {
            return Self::Lsp(event);
        }
        Self::runtime_event(event).unwrap_or(Self::Application)
    }

    fn runtime_event(event: &EventEnvelope) -> Option<Self> {
        if event.downcast_ref::<WorkerStopped>().is_some() {
            return Some(Self::Fail("review worker stopped unexpectedly".into()));
        }
        if let Some(TerminalFailed(message)) = event.downcast_ref::<TerminalFailed>() {
            return Some(Self::Fail(format!(
                "could not read terminal input: {message}"
            )));
        }
        if event.downcast_ref::<StopRequested>().is_some() {
            return Some(Self::Stop);
        }
        if event.downcast_ref::<RepositoryRefreshDue>().is_some() {
            return Some(Self::RefreshDue);
        }
        if let Some(InputAcknowledged(id)) = event.downcast_ref::<InputAcknowledged>() {
            return Some(Self::Acknowledged(*id));
        }
        event
            .downcast_ref::<TerminalFocused>()
            .map(|_| Self::TerminalFocused)
    }

    /// Whether handling the event can change what the screen shows.
    ///
    /// Agent detection belongs to the delivery worker; the UI events it causes
    /// redraw on their own.
    pub(super) fn needs_frame(&self) -> bool {
        !matches!(self, Self::Herdr(HerdrEvent::AgentDetected { .. }))
    }

    /// Whether the terminal may have shown or hidden the cursor behind our back.
    pub(super) fn resets_cursor(&self) -> bool {
        matches!(
            self,
            Self::Input(_) | Self::TerminalFocused | Self::AgentFocused(_)
        )
    }
}

#[cfg(test)]
#[path = "route.tests.rs"]
mod tests;
