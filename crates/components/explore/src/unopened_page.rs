//! The Explore page the browser could not open, for Start, Start with Challenger or Open the
//! Explore page: the pane says why and gives the page's address to open by hand, until the next
//! start or the next try.

use ui_events::ExplorePageNotOpened;
use ui_theme::Palette;

use super::{ExploreComponent, flow::ConversationLayout};

impl ExploreComponent {
    pub(super) fn page_not_opened(&mut self, event: &ExplorePageNotOpened) {
        self.unopened_page = Some(event.0.clone());
    }

    /// Why the browser did not open the page, and the address to open it at.
    pub(super) fn lay_out_unopened_page(&self, layout: &mut ConversationLayout, palette: Palette) {
        let Some(page) = &self.unopened_page else {
            return;
        };
        layout.gap();
        layout.text(page.to_string(), palette.warning, None);
        if let Some(url) = &page.url {
            layout.text("Open it in a browser on this machine:", palette.dim, None);
            layout.text(url.clone(), palette.text, None);
        }
    }
}
