//! A round on the Explore page: started with Start or Start with Challenger, or on the page,
//! the pane shows no interview, only that the round runs on the page, its state, a button that
//! opens the page again, and the page's address. "Continue in the pane" shows the interview.

use super::recovery::{restore, round, saved};
use super::*;
use crate::ExplorePageAction;
use review_explore::{ExploreViewState, RoundFront, ViewSave};

const RUNS_ON_PAGE: &str = "runs on the Explore page";

impl ExploreUi {
    /// Whether the pane shows the round as a round on the page.
    fn shows_page_round(&self) -> bool {
        let text = self.text();
        text.contains(RUNS_ON_PAGE)
            && text.contains(" Open the Explore page ")
            && text.contains(" Continue in the pane ")
    }
}

#[test]
fn a_round_started_with_start_runs_on_the_page_and_the_pane_shows_no_interview() {
    for start in ['s', 'S'] {
        let (mut fixture, request) = ExploreUi::started(BASE, POLICY, start);
        assert!(fixture.shows_page_round(), "{}", fixture.text());
        assert!(fixture.text().contains("The agent is working"));

        fixture.respond(&request, 1);

        let text = fixture.text();
        assert!(fixture.shows_page_round(), "{text}");
        assert!(text.contains("A question waits for your answer"), "{text}");
        assert!(!text.contains("Question 1"), "{text}");
        assert!(!text.contains("Keep resolved"), "{text}");
        assert!(text.contains(" Reset "), "Reset stays");
    }
}

#[test]
fn a_round_started_in_the_pane_shows_the_interview() {
    let (mut fixture, request) = ExploreUi::started(BASE, POLICY, 'p');

    fixture.respond(&request, 1);

    assert!(!fixture.text().contains(RUNS_ON_PAGE));
    assert!(fixture.text().contains("Question 1"));
}

#[test]
fn the_page_round_view_opens_the_page_again_and_ignores_interview_keys() {
    let (mut fixture, request) = ExploreUi::started(BASE, POLICY, 's');
    fixture.respond(&request, 1);

    let opened = fixture.click_actions(" Open the Explore page ");
    assert!(opened.contains(&Action::ExplorePage(ExplorePageAction::Open)));

    for key in [
        Key::Char('2'),
        Key::Enter,
        Key::Char('x'),
        Key::ControlEnter,
    ] {
        let actions = fixture.app.update(UserInput::Key(key));
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, Action::Explore(Command::Turn(_)))),
            "{key:?} sends nothing"
        );
    }
    fixture.click(" Continue in the pane ");
    let text = fixture.text();
    assert!(text.contains("Question 1"), "{text}");
    assert!(
        !text.contains("› 2. Inspect the caller"),
        "no choice was made"
    );
}

#[test]
fn the_keys_of_the_page_round_buttons_show_in_the_footer_and_act() {
    let (mut fixture, request) = ExploreUi::started(BASE, POLICY, 's');
    fixture.respond(&request, 1);
    let footer = fixture.footer();
    assert!(footer.contains("o ") && footer.contains("i "), "{footer:?}");

    let opened = fixture.app.update(UserInput::Key(Key::Char('o')));
    assert!(opened.contains(&Action::ExplorePage(ExplorePageAction::Open)));
    fixture.app.update(UserInput::Key(Key::Char('i')));
    assert!(fixture.text().contains("Question 1"));
    assert!(
        fixture
            .app
            .update(UserInput::Key(Key::Char('o')))
            .is_empty(),
        "a round in the pane opens nothing"
    );
}

#[test]
fn continue_in_the_pane_is_kept_with_the_saved_view_across_reopening() {
    let (mut fixture, request) = ExploreUi::started(BASE, POLICY, 's');
    let round = round(&fixture, &request);
    let on_page = saved(restore(
        &mut fixture,
        &round,
        Some(ViewSave {
            instance: round.exploration.instance.clone(),
            review_unit: round.exploration.comparison.checkpoint.review_unit.clone(),
            sequence: 1,
            state: ExploreViewState {
                front: RoundFront::Page,
                ..Default::default()
            },
        }),
    ));
    assert!(fixture.shows_page_round(), "{}", fixture.text());
    assert_eq!(on_page.state.front, RoundFront::Page);

    let in_pane = saved(fixture.click_actions(" Continue in the pane "));
    assert_eq!(in_pane.state.front, RoundFront::Pane);

    restore(&mut fixture, &round, Some(in_pane));
    assert!(fixture.text().contains("Question 1"));
    assert!(!fixture.text().contains(RUNS_ON_PAGE));
}

#[test]
fn continue_in_the_pane_while_the_round_starts_opens_nothing() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);
    fixture.app.update(UserInput::Key(Key::Char('s')));
    assert!(fixture.shows_page_round(), "{}", fixture.text());

    fixture.app.update(UserInput::Key(Key::Char('i')));
    let captured = fixture.app.publish(ExploreCaptured {
        result: Ok(fixture.comparison.clone()),
    });

    assert!(!captured.contains(&Action::ExplorePage(ExplorePageAction::Open)));
    let request = ExploreUi::request(captured);
    fixture.respond(&request, 1);
    assert!(fixture.text().contains("Question 1"));
}

#[test]
fn a_saved_round_without_a_saved_front_shows_in_the_pane() {
    let (mut fixture, request) = ExploreUi::started(BASE, POLICY, 's');
    let round = round(&fixture, &request);

    restore(&mut fixture, &round, None);

    assert!(fixture.text().contains("Question 1"));
}

#[test]
fn the_state_line_follows_the_round() {
    let (mut fixture, request) = ExploreUi::started(BASE, POLICY, 's');
    let mut response = fixture.response(&request, 1);
    response.request = "another request".into();
    fixture.app.publish(ExploreFinished {
        instance: request.instance.clone(),
        request: request.request.clone(),
        result: Ok(response),
    });
    let text = fixture.text();
    assert!(text.contains("Interrupted"), "{text}");
    assert!(text.contains("Response identity"), "the reason: {text}");

    let retry = fixture.app.update(UserInput::Key(Key::Char('r')));
    assert!(
        !retry
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::Retry(_)))),
        "Retry is on the page, or in the pane after Continue in the pane"
    );
}

#[test]
fn a_concluded_round_on_the_page_says_so() {
    let (mut fixture, request) = ExploreUi::started(BASE, POLICY, 's');
    let mut update = fixture.response(&request, 1);
    update.next = None;
    update.conclusion = Some(conclusion("Nothing else to ask."));
    fixture.app.publish(ExploreFinished {
        instance: request.instance.clone(),
        request: request.request.clone(),
        result: Ok(update),
    });

    let text = fixture.text();
    assert!(text.contains("Concluded"), "{text}");
    assert!(!text.contains("Nothing else to ask."), "{text}");
}
