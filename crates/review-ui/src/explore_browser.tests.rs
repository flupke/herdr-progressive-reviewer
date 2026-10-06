//! Starting a round from the pane: Start and Start with Challenger also open the round's
//! Explore page in the browser, Start in the pane and Start in the pane with Challenger do not.

use super::*;
use crate::ExplorePageAction;
use review_explore_page_opening::PageNotOpened;

const PAGE: &str = "http://127.0.0.1:41234/?token=0123456789abcdef0123456789abcdef";

/// One way to start a round from the start screen.
#[derive(Clone, Copy, Debug)]
enum Trigger {
    Key(char),
    Button(&'static str),
}

/// The four starts: how to trigger each, whether it has a Challenger, and whether it opens
/// the page.
const STARTS: [(Trigger, bool, bool); 8] = [
    (Trigger::Key('s'), false, true),
    (Trigger::Key('S'), true, true),
    (Trigger::Key('p'), false, false),
    (Trigger::Key('P'), true, false),
    // Two spaces: the status line above it reads " Start a question-first review".
    (Trigger::Button(" Start  "), false, true),
    (Trigger::Button(" Start with Challenger "), true, true),
    (Trigger::Button(" Start in the pane "), false, false),
    (
        Trigger::Button(" Start in the pane with Challenger "),
        true,
        false,
    ),
];

fn opens_page(actions: &[Action]) -> bool {
    actions.contains(&Action::ExplorePage(ExplorePageAction::Open))
}

impl ExploreUi {
    /// Start a round with `trigger`; returns the actions of the start and of the capture.
    fn start_with(&mut self, trigger: Trigger) -> (Vec<Action>, Vec<Action>) {
        let started = match trigger {
            Trigger::Key(key) => self.app.update(UserInput::Key(Key::Char(key))),
            Trigger::Button(label) => self.click_actions(label),
        };
        let captured = self.app.publish(ExploreCaptured {
            result: Ok(self.comparison.clone()),
        });
        (started, captured)
    }
}

#[test]
fn only_start_and_start_with_challenger_open_the_page_once_the_change_is_captured() {
    for (trigger, challenger, opens) in STARTS {
        let mut fixture = ExploreUi::start_screen(BASE, POLICY);

        let (started, captured) = fixture.start_with(trigger);

        assert!(
            started.contains(&Action::Explore(Command::Start)),
            "{trigger:?} starts a round"
        );
        assert!(!opens_page(&started), "{trigger:?} waits for the capture");
        assert_eq!(opens_page(&captured), opens, "{trigger:?}");
        let kickoff = ExploreUi::request(captured);
        assert_eq!(kickoff.challenger, challenger, "{trigger:?}");
    }
}

#[test]
fn retry_after_a_failed_capture_opens_nothing() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);
    fixture.app.update(UserInput::Key(Key::Char('s')));
    fixture.app.publish(ExploreCaptured {
        result: Err("Repository comparison is not ready; retry Start".into()),
    });

    let retried = fixture.app.update(UserInput::Key(Key::Char('r')));
    let captured = fixture.app.publish(ExploreCaptured {
        result: Ok(fixture.comparison.clone()),
    });

    assert!(retried.contains(&Action::Explore(Command::Start)));
    assert!(!opens_page(&retried) && !opens_page(&captured));
    ExploreUi::request(captured);
    assert!(
        fixture.text().contains("runs on the Explore page"),
        "the round Start began stays on the page"
    );
}

#[test]
fn a_reset_and_a_new_round_in_the_pane_open_nothing() {
    let (mut fixture, request) = ExploreUi::started(BASE, POLICY, 's');
    fixture.respond(&request, 1);

    let reset = fixture.reset();
    let (started, captured) = fixture.start_with(Trigger::Key('p'));

    assert!(!opens_page(&reset) && !opens_page(&started) && !opens_page(&captured));
}

#[test]
fn the_footer_of_the_start_screen_shows_the_keys_of_the_four_starts() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);

    let footer = fixture.footer();
    for key in ["s ", "S ", "p ", "P "] {
        assert!(footer.contains(key), "{key:?} missing from {footer:?}");
    }
    assert!(footer.contains("? help"));

    fixture
        .app
        .publish(ReviewNavigationChanged(ReviewNavigation::Files));
    assert_eq!(
        fixture.footer().trim_end(),
        "? help",
        "Files has no start keys"
    );

    fixture
        .app
        .publish(ReviewNavigationChanged(ReviewNavigation::Explore));
    fixture.start_with(Trigger::Key('p'));
    assert_eq!(
        fixture.footer().trim_end(),
        "? help",
        "a round in the pane has no start keys"
    );
}

#[test]
fn a_page_the_browser_could_not_open_shows_why_and_its_address_until_the_next_start() {
    const REASON: &str = "xdg-open failed (exit status: 3)";
    let (mut fixture, request) = ExploreUi::started(BASE, POLICY, 's');

    fixture
        .app
        .publish(ui_events::ExplorePageNotOpened(PageNotOpened {
            url: Some(PAGE.into()),
            reason: REASON.into(),
        }));
    assert!(fixture.text().contains(REASON), "{}", fixture.text());
    assert!(fixture.text().contains(PAGE));

    fixture.respond(&request, 1);
    fixture.app.update(UserInput::MouseScroll {
        column: 0,
        row: 4,
        delta: 500,
    });
    assert!(fixture.text().contains(REASON), "the round still shows it");
    assert!(fixture.text().contains(PAGE));

    fixture.reset();
    fixture.start_with(Trigger::Key('p'));
    assert!(!fixture.text().contains(REASON), "a new start forgets it");
}

#[test]
fn a_click_on_the_address_of_a_page_the_browser_could_not_open_copies_it() {
    let (mut fixture, _) = ExploreUi::started(BASE, POLICY, 's');
    fixture
        .app
        .publish(ui_events::ExplorePageNotOpened(PageNotOpened {
            url: Some(PAGE.into()),
            reason: "xdg-open failed (exit status: 3)".into(),
        }));

    assert_eq!(
        fixture.click_actions("http://127.0.0.1:41234"),
        [Action::Terminal(crate::TerminalAction::CopyToClipboard(
            PAGE.into()
        ))]
    );
    assert!(
        fixture.text().contains("Copied the page's address"),
        "{}",
        fixture.text()
    );
}
