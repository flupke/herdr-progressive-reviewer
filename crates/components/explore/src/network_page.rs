//! The Explore page on the network: its address and QR code, for a phone or a tablet on the same
//! network. While no round runs, it opens the page that starts the next round.

use ui_qr_code::QrCode;
use ui_theme::Palette;

use super::{ExploreComponent, flow::ConversationLayout};

/// The address of the page on the network, and its QR code.
pub(super) struct NetworkPage {
    url: String,
    /// `None` when the address is too long for a QR code.
    code: Option<QrCode>,
}

impl ExploreComponent {
    pub(super) fn page_shared(&mut self, event: &ui_events::ExplorePageShared) {
        let url = event.0.clone();
        self.network_page = Some(NetworkPage {
            code: QrCode::new(&url),
            url,
        });
    }

    /// The end of each page of the pane: the address of the page on the network, and its QR
    /// code when the pane is wide enough to draw it.
    pub(super) fn lay_out_network_page(&self, layout: &mut ConversationLayout, palette: Palette) {
        let Some(page) = &self.network_page else {
            return;
        };
        layout.gap();
        let invitation = if self.exploration.is_some() {
            "Open this round on a phone on the same network:"
        } else {
            "Start a round from a phone on the same network:"
        };
        layout.text(invitation, palette.dim, None);
        layout.text(page.url.clone(), palette.text, None);
        if let Some(code) = page
            .code
            .as_ref()
            .filter(|code| code.width() <= layout.area.width)
        {
            layout.picture(code.text(), code.height());
        }
    }
}
