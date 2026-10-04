//! The Explore page on the network: its address and QR code, for a phone or a tablet on the same
//! network. While no round runs, it opens the page that starts the next round. When the page
//! could not be served on the network, one line says so in their place.

use ui_qr_code::QrCode;
use ui_theme::Palette;

use super::{ExploreComponent, flow::ConversationLayout};

/// The page on the network, as the page host announced it.
pub(super) enum NetworkPage {
    /// The address of the page on the network, and its QR code.
    Shared {
        url: String,
        /// `None` when the address is too long for a QR code.
        code: Option<QrCode>,
    },
    /// The page could not be served on the network, for this reason.
    NotShared(String),
}

impl ExploreComponent {
    pub(super) fn page_shared(&mut self, event: &ui_events::ExplorePageShared) {
        let url = event.0.clone();
        self.network_page = Some(NetworkPage::Shared {
            code: QrCode::new(&url),
            url,
        });
    }

    pub(super) fn page_not_shared(&mut self, event: &ui_events::ExplorePageNotShared) {
        self.network_page = Some(NetworkPage::NotShared(event.0.clone()));
    }

    /// The end of each page of the pane: the address of the page on the network, and its QR
    /// code when the pane is wide enough to draw it, or why the page is not on the network.
    pub(super) fn lay_out_network_page(&self, layout: &mut ConversationLayout, palette: Palette) {
        let Some(page) = &self.network_page else {
            return;
        };
        layout.gap();
        let (url, code) = match page {
            NetworkPage::Shared { url, code } => (url, code),
            NetworkPage::NotShared(reason) => {
                layout.text(
                    format!("The page is not shared on the network: {reason}"),
                    palette.dim,
                    None,
                );
                return;
            }
        };
        let invitation = if self.exploration.is_some() {
            "Open this round on a phone on the same network:"
        } else {
            "Start a round from a phone on the same network:"
        };
        layout.text(invitation, palette.dim, None);
        layout.text(url.clone(), palette.text, None);
        if let Some(code) = code
            .as_ref()
            .filter(|code| code.width() <= layout.area.width)
        {
            layout.picture(code.text(), code.height());
        }
    }
}
