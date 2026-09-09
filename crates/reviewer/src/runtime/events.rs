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
