//! The QR code and the address of the round's page on the network, in the pane.

use super::*;

const URL: &str = "http://192.168.1.23:8790/?token=0123456789abcdef0123456789abcdef";

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
        .publish(ui_events::ExplorePageShared(Some(URL.into())));
    assert!(fixture.text().contains(URL));
    assert!(fixture.shows_qr_code());

    fixture.respond(&request, 1);
    fixture.scroll_to_end();
    assert!(fixture.text().contains(URL));
    assert!(fixture.shows_qr_code());

    fixture.reset();
    assert!(!fixture.text().contains(URL), "no round is running");
    assert!(!fixture.shows_qr_code());
}

#[test]
fn the_qr_code_leaves_once_the_round_has_no_page_on_the_network() {
    let (mut fixture, _) = ExploreUi::new();
    fixture
        .app
        .publish(ui_events::ExplorePageShared(Some(URL.into())));

    fixture.app.publish(ui_events::ExplorePageShared(None));

    assert!(!fixture.text().contains(URL));
    assert!(!fixture.shows_qr_code());
}
