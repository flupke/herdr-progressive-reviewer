use component_core::EventEnvelope;
use crossbeam_channel::{Receiver, RecvError, TryRecvError, never, select_biased};

pub(super) struct Inbox {
    interactive: Option<Receiver<EventEnvelope>>,
    background: Option<Receiver<EventEnvelope>>,
}

impl Inbox {
    pub(super) fn new(
        background: Receiver<EventEnvelope>,
        interactive: Receiver<EventEnvelope>,
    ) -> Self {
        Self {
            interactive: Some(interactive),
            background: Some(background),
        }
    }

    pub(super) fn recv(&mut self) -> Result<EventEnvelope, RecvError> {
        loop {
            if let Some(event) = self.try_recv() {
                return Ok(event);
            }
            if self.interactive.is_none() && self.background.is_none() {
                return Err(RecvError);
            }
            let disconnected = never();
            select_biased! {
                recv(self.interactive.as_ref().unwrap_or(&disconnected)) -> event => {
                    match event {
                        Ok(event) => return Ok(event),
                        Err(_) => self.interactive = None,
                    }
                },
                recv(self.background.as_ref().unwrap_or(&disconnected)) -> event => {
                    match event {
                        Ok(event) => return Ok(event),
                        Err(_) => self.background = None,
                    }
                },
            }
        }
    }

    /// The next event, or `None` once `guard` ran out with none: for tests, whose guard only
    /// ends a wait that failed.
    #[cfg(test)]
    pub(super) fn recv_within(&mut self, guard: std::time::Duration) -> Option<EventEnvelope> {
        let deadline = std::time::Instant::now() + guard;
        loop {
            if let Some(event) = self.try_recv() {
                return Some(event);
            }
            if self.interactive.is_none() && self.background.is_none() {
                return None;
            }
            let disconnected = never();
            select_biased! {
                recv(self.interactive.as_ref().unwrap_or(&disconnected)) -> event => {
                    match event {
                        Ok(event) => return Some(event),
                        Err(_) => self.interactive = None,
                    }
                },
                recv(self.background.as_ref().unwrap_or(&disconnected)) -> event => {
                    match event {
                        Ok(event) => return Some(event),
                        Err(_) => self.background = None,
                    }
                },
                default(deadline.saturating_duration_since(std::time::Instant::now())) => return None,
            }
        }
    }

    /// Waits until an event is queued, without taking it; false once `guard` ran out: for
    /// tests that let the event loop take it.
    #[cfg(test)]
    pub(super) fn wait_until_queued(&self, guard: std::time::Duration) -> bool {
        let mut select = crossbeam_channel::Select::new();
        for receiver in [&self.interactive, &self.background].into_iter().flatten() {
            select.recv(receiver);
        }
        select.ready_timeout(guard).is_ok()
    }

    pub(super) fn try_recv(&mut self) -> Option<EventEnvelope> {
        Self::take(&mut self.interactive).or_else(|| Self::take(&mut self.background))
    }

    fn take(receiver: &mut Option<Receiver<EventEnvelope>>) -> Option<EventEnvelope> {
        match receiver.as_ref()?.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Disconnected) => {
                *receiver = None;
                None
            }
            Err(TryRecvError::Empty) => None,
        }
    }
}
