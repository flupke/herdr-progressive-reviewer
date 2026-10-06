//! The QR code and the address of the Explore page on the network, in the pane.

use super::*;

const URL: &str = "http://192.168.1.23:8790/?token=0123456789abcdef0123456789abcdef";
/// The address the page gets once the round is reset, for the round that starts next.
const NEXT_URL: &str = "http://192.168.1.23:8790/?token=fedcba9876543210fedcba9876543210";

impl ExploreUi {
    /// Scrolls the conversation to its end.
    fn scroll_to_end(&mut self) {
        self.app.update(UserInput::MouseScroll {
            column: 0,
            row: 4,
            delta: 500,
        });
    }

    /// Whether the screen shows a QR code: rows of cells that each draw two of its modules.
    fn shows_qr_code(&self) -> bool {
        self.text().contains(&"▀".repeat(30))
    }
}

#[test]
fn a_running_round_shows_the_qr_code_and_the_address_of_its_page() {
    let (mut fixture, request) = ExploreUi::new();
    assert!(
        !fixture.shows_qr_code(),
        "the page is not on the network yet"
    );

    fixture
        .app
        .publish(ui_events::ExplorePageShared(URL.into()));
    assert!(fixture.text().contains(URL));
    assert!(fixture.shows_qr_code());

    fixture.respond(&request, 1);
    fixture.scroll_to_end();
    assert!(fixture.text().contains(URL));
    assert!(fixture.shows_qr_code());
}

#[test]
fn the_start_screen_shows_the_address_of_the_page_that_starts_the_next_round() {
    let (mut fixture, _) = ExploreUi::new();
    fixture
        .app
        .publish(ui_events::ExplorePageShared(URL.into()));

    fixture.reset();
    fixture
        .app
        .publish(ui_events::ExplorePageShared(NEXT_URL.into()));

    assert!(fixture.text().contains(NEXT_URL));
    assert!(fixture.shows_qr_code());
    fixture.app.publish(ui_events::ExploreRestored {
        result: Ok(None),
        view: None,
        historical: false,
        storage_error: None,
        progress: ui_events::ExploreProgress::Ready,
        turn_paths: std::collections::BTreeMap::new(),
    });
    assert!(
        fixture.text().contains(NEXT_URL),
        "a reopened start screen keeps the address"
    );
}

#[test]
fn a_page_that_cannot_be_shared_says_why_in_place_of_the_address() {
    const REASON: &str = "Network is unreachable (os error 101)";
    let (mut fixture, request) = ExploreUi::new();

    fixture
        .app
        .publish(ui_events::ExplorePageNotShared(REASON.into()));
    assert!(fixture.text().contains(REASON), "{}", fixture.text());
    assert!(!fixture.shows_qr_code());

    fixture.respond(&request, 1);
    fixture.scroll_to_end();
    assert!(fixture.text().contains(REASON), "{}", fixture.text());
}

#[test]
fn a_click_on_the_address_or_its_qr_code_copies_the_address() {
    let (mut fixture, _) = ExploreUi::new();
    fixture
        .app
        .publish(ui_events::ExplorePageShared(URL.into()));
    let copy = [Action::Terminal(crate::TerminalAction::CopyToClipboard(
        URL.into(),
    ))];

    assert_eq!(fixture.click_actions("http://192.168.1.23"), copy);
    assert!(
        fixture.text().contains("Copied the page's address"),
        "{}",
        fixture.text()
    );
    assert_eq!(fixture.click_actions(&"▀".repeat(30)), copy);
}
