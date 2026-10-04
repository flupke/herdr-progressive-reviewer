//! The Explore page on the network, as the settings say: shared on the listener they choose
//! when the reviewer starts, moved when they change, and taken off the network when they turn
//! network access off. The pane hears of each: the page's address, why it is not on the
//! network, or that it left it.

use std::sync::{Mutex, PoisonError};

use component_core::EventEnvelope;
use crossbeam_channel::Sender as EventSender;
use review_explore_page_host::{NetworkAccess, NetworkListener, PageNetwork};

/// Applies the network settings to the page of one reviewer.
pub(super) struct PageSharing {
    network: PageNetwork,
    /// Where the pane hears of the page on the network.
    events: EventSender<EventEnvelope>,
    /// The settings applied last; `None` before the first, and after settings that failed.
    applied: Mutex<Option<NetworkAccess>>,
}

impl PageSharing {
    pub(super) fn new(network: PageNetwork, events: EventSender<EventEnvelope>) -> Self {
        Self {
            network,
            events,
            applied: Mutex::new(None),
        }
    }

    /// Serves the page on the network as `access` says, unless it already does: on a new
    /// listener in place of the earlier one, or nowhere. Settings that could not be applied are
    /// tried again the next time. A listener that cannot start is not an error toast: without a
    /// network, or with the ports busy, it would show at every start. The pane says it in place
    /// of the address.
    pub(super) fn apply(&self, access: &NetworkAccess) {
        let mut applied = self.applied.lock().unwrap_or_else(PoisonError::into_inner);
        if applied.as_ref() == Some(access) {
            return;
        }
        // No address of the earlier listener is announced after this, and its port is free.
        self.network.unshare();
        // Settings that failed are tried again when they are applied again.
        *applied = None;
        match NetworkListener::bind(access) {
            Ok(Some(listener)) => {
                let events = self.events.clone();
                self.network.share(listener, move |url| {
                    let shared = ui_events::ExplorePageShared(url.to_owned());
                    let _ = events.send(EventEnvelope::new(shared));
                });
                *applied = Some(access.clone());
            }
            Ok(None) => {
                self.send(ui_events::ExplorePageOffNetwork);
                *applied = Some(access.clone());
            }
            Err(error) => self.send(ui_events::ExplorePageNotShared(error.to_string())),
        }
    }

    fn send<E: component_core::ApplicationEvent>(&self, event: E) {
        let _ = self.events.send(EventEnvelope::new(event));
    }
}
