//! Sharing the running round over a tunnel from the pane: its switch with the Explore page
//! settings, then its link and QR code, or why it is not there.

use super::*;
use crate::ExplorePageAction;
use review_explore_page_tunnel::TunnelState;
use ui_events::ExplorePageTunnel;

const LINK: &str =
    "https://quiet-river-stone-lamp.trycloudflare.com/?token=0123456789abcdef0123456789abcdef";

impl ExploreUi {
    /// Turns the tunnel's switch over with its key, and returns what the pane asks for.
    fn share_key(&mut self) -> Vec<Action> {
        self.app.update(UserInput::Key(Key::Char('O')))
    }

    /// The pane, scrolled to its end, where the tunnel's link shows.
    fn end(&mut self) -> String {
        self.app.update(UserInput::MouseScroll {
            column: 0,
            row: 4,
            delta: 500,
        });
        self.text()
    }

    fn report(&mut self, state: TunnelState) {
        self.app.publish(ExplorePageTunnel(state));
    }
}

/// Whether `text` shows a QR code: rows of cells that each draw two of its modules.
fn shows_qr_code(text: &str) -> bool {
    text.contains(&"▀".repeat(30))
}

#[test]
fn the_reviewer_shares_the_running_round_and_gets_its_link_and_qr_code() {
    let (mut fixture, _) = ExploreUi::new();
    assert!(fixture.end().contains("Share over a tunnel: off"));

    let actions = fixture.share_key();

    assert_eq!(
        actions,
        [Action::ExplorePage(ExplorePageAction::OpenTunnel)]
    );
    assert!(fixture.end().contains("Share over a tunnel: opening"));
    fixture.report(TunnelState::Opening);
    fixture.report(TunnelState::Open { url: LINK.into() });
    let text = fixture.end();
    assert!(text.contains("Share over a tunnel: on"), "{text}");
    assert!(text.contains(LINK), "{text}");
    assert!(shows_qr_code(&text), "{text}");
}

#[test]
fn what_the_switch_did_shows_right_under_the_settings_before_the_phones_page() {
    const PHONE: &str = "http://192.168.1.23:8790/?token=0123456789abcdef0123456789abcdef";
    let (mut fixture, _) = ExploreUi::new();
    fixture
        .app
        .publish(ui_events::ExplorePageShared(PHONE.into()));
    fixture.share_key();

    fixture.report(TunnelState::Open { url: LINK.into() });

    let text = fixture.end();
    let switch = text.find("Share over a tunnel: on").unwrap();
    let link = text.find(LINK).unwrap();
    let phone = text.find(PHONE).unwrap();
    assert!(switch < link && link < phone, "{text}");
}

#[test]
fn turning_the_tunnel_off_stops_it_and_takes_its_link_away() {
    let (mut fixture, _) = ExploreUi::new();
    fixture.share_key();
    fixture.report(TunnelState::Open { url: LINK.into() });

    let actions = fixture.share_key();

    assert_eq!(
        actions,
        [Action::ExplorePage(ExplorePageAction::CloseTunnel)]
    );
    let text = fixture.end();
    assert!(text.contains("Share over a tunnel: off"), "{text}");
    assert!(!text.contains(LINK), "{text}");
}

#[test]
fn a_tunnel_that_did_not_open_says_why_and_can_be_tried_again() {
    const REASON: &str =
        "cloudflared is not installed, install cloudflared from your package manager";
    let (mut fixture, _) = ExploreUi::new();
    fixture.share_key();

    fixture.report(TunnelState::Failed(REASON.into()));

    let text = fixture.end();
    assert!(text.contains(REASON), "{text}");
    assert!(text.contains("Share over a tunnel: off"), "{text}");
    assert_eq!(
        fixture.share_key(),
        [Action::ExplorePage(ExplorePageAction::OpenTunnel)]
    );
}

#[test]
fn the_link_goes_when_the_tunnel_ends_with_its_round() {
    let (mut fixture, _) = ExploreUi::new();
    fixture.share_key();
    fixture.report(TunnelState::Open { url: LINK.into() });

    fixture.reset();
    fixture.report(TunnelState::Off);

    let text = fixture.end();
    assert!(!text.contains(LINK), "{text}");
    assert!(text.contains("Share over a tunnel: off"), "{text}");
}
