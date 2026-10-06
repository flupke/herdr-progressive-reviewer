//! The Explore page on the network: its address and QR code, for a phone or a tablet on the same
//! network. While no round runs, it opens the page that starts the next round. When the page
//! could not be served on the network, one line says so in their place. Above it, right under the
//! settings and their switch, while the reviewer shares the running round over a tunnel, the
//! tunnel's link and its QR code, or one line that says why the tunnel is not there. A click on
//! an address or its QR code copies the address to the clipboard.

use review_explore_page_tunnel::TunnelState;
use toasts::ToastKind;
use ui_actions::{Action, TerminalAction};
use ui_events::ToastRequested;
use ui_qr_code::QrCode;
use ui_theme::Palette;

use super::{Control, ExploreComponent, flow::ConversationLayout};

/// Which of the pane's addresses of the Explore page a click copies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SharedLink {
    /// The page's address on this machine, which the browser could not open.
    Local,
    /// The address of the page on the network.
    Network,
    /// The link of the tunnel that shares the running round.
    Tunnel,
}

/// An address that opens the page, with its QR code.
pub(super) struct PageLink {
    url: String,
    /// `None` when the address is too long for a QR code.
    code: Option<QrCode>,
}

impl PageLink {
    fn new(url: String) -> Self {
        Self {
            code: QrCode::new(&url),
            url,
        }
    }

    /// The address, underlined as a link, then its QR code when the pane is wide enough to
    /// draw it; a click on either copies the address.
    fn lay_out(&self, layout: &mut ConversationLayout, palette: Palette, link: SharedLink) {
        layout.link(self.url.clone(), palette.text, Control::CopyLink(link));
        if let Some(code) = self
            .code
            .as_ref()
            .filter(|code| code.width() <= layout.area.width)
        {
            layout.picture(code.text(), code.height(), Some(Control::CopyLink(link)));
        }
    }
}

/// The page on the network, as the page host announced it.
pub(super) enum NetworkPage {
    /// The address of the page on the network.
    Shared(PageLink),
    /// The page could not be served on the network, for this reason.
    NotShared(String),
}

/// The tunnel that shares the running round, as the page host reported it, with its link while
/// it is open.
#[derive(Default)]
pub(super) struct TunnelView {
    state: TunnelState,
    link: Option<PageLink>,
}

impl TunnelView {
    pub(super) fn new(state: TunnelState) -> Self {
        let link = match &state {
            TunnelState::Open { url } => Some(PageLink::new(url.clone())),
            _ => None,
        };
        Self { state, link }
    }

    pub(super) fn state(&self) -> &TunnelState {
        &self.state
    }
}

impl ExploreComponent {
    pub(super) fn page_shared(&mut self, event: &ui_events::ExplorePageShared) {
        self.network_page = Some(NetworkPage::Shared(PageLink::new(event.0.clone())));
    }

    pub(super) fn page_not_shared(&mut self, event: &ui_events::ExplorePageNotShared) {
        self.network_page = Some(NetworkPage::NotShared(event.0.clone()));
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn page_off_network(&mut self, _: &ui_events::ExplorePageOffNetwork) {
        self.network_page = None;
    }

    /// The end of each page of the pane: the address of the page on the network, and its QR
    /// code when the pane is wide enough to draw it, or why the page is not on the network.
    pub(super) fn lay_out_network_page(&self, layout: &mut ConversationLayout, palette: Palette) {
        let Some(page) = &self.network_page else {
            return;
        };
        layout.gap();
        let link = match page {
            NetworkPage::Shared(link) => link,
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
        link.lay_out(layout, palette, SharedLink::Network);
    }

    /// Copies the address `link` names, while the pane shows it.
    pub(super) fn copy_link(&self, link: SharedLink) -> Vec<Action> {
        let (url, copied) = match link {
            SharedLink::Local => match self
                .unopened_page
                .as_ref()
                .and_then(|page| page.url.as_ref())
            {
                Some(url) => (url, "the page's address"),
                None => return Vec::new(),
            },
            SharedLink::Network => match &self.network_page {
                Some(NetworkPage::Shared(page_link)) => (&page_link.url, "the page's address"),
                _ => return Vec::new(),
            },
            SharedLink::Tunnel => match &self.page_settings.tunnel().link {
                Some(page_link) => (&page_link.url, "the tunnel's link"),
                None => return Vec::new(),
            },
        };
        self.events.publish(ToastRequested {
            text: format!("Copied {copied} to the clipboard"),
            kind: ToastKind::Info,
        });
        vec![Action::Terminal(TerminalAction::CopyToClipboard(
            url.clone(),
        ))]
    }

    /// Right under the settings, where the reviewer sees what the switch did: the link of the
    /// tunnel that shares the running round, with what it gives, and its QR code when the pane
    /// is wide enough to draw it; that it opens; or why it is not there.
    pub(super) fn lay_out_tunnel(&self, layout: &mut ConversationLayout, palette: Palette) {
        let tunnel = self.page_settings.tunnel();
        let (line, color) = match tunnel.state() {
            TunnelState::Off => return,
            TunnelState::Opening => ("Opening a tunnel with cloudflared…".into(), palette.dim),
            TunnelState::Failed(reason) => (
                format!("Cannot share the round over a tunnel: {reason}"),
                palette.warning,
            ),
            TunnelState::Open { .. } => (
                "Share this round: anyone with this link can use it as you, Implement included:"
                    .into(),
                palette.warning,
            ),
        };
        layout.gap();
        layout.text(line, color, None);
        if let Some(link) = &tunnel.link {
            link.lay_out(layout, palette, SharedLink::Tunnel);
        }
    }
}
