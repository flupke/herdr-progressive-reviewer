//! The settings for the Explore rounds in the Explore tab: shown with their values on every
//! screen, changed in place, saved with the reviewer's other settings, and taken by the round
//! started next (the writing style) or by the next question (run-ahead).

use super::*;
use review_explore_round_settings::{ExploreRoundSettings, RunAhead, WritingStyle};
use ui_events::ExploreRoundSettingsLoaded;

/// The writing style that `actions` save, if they save one.
fn saved(actions: &[Action]) -> Option<WritingStyle> {
    actions.iter().find_map(|action| match action {
        Action::Settings(SettingsAction::SaveExploreWritingStyle(writing)) => Some(*writing),
        _ => None,
    })
}

/// The kickoff of the round started in the pane from the start screen of `fixture`.
fn start_in_pane(fixture: &mut ExploreUi) -> TurnRequest {
    fixture.app.update(UserInput::Key(Key::Char('p')));
    let captured = fixture.app.publish(ExploreCaptured {
        result: Ok(fixture.comparison.clone()),
    });
    ExploreUi::request(captured)
}

#[test]
fn the_next_round_is_written_in_simplified_technical_english_by_default() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);

    let text = fixture.text();
    assert!(
        text.contains("Writing style of the next round: Simplified Technical English"),
        "{text}"
    );
    assert_eq!(
        start_in_pane(&mut fixture).writing,
        WritingStyle::SimplifiedTechnicalEnglish
    );
}

#[test]
fn the_writing_style_key_saves_the_other_style_and_the_next_round_takes_it() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);

    let actions = fixture.app.update(UserInput::Key(Key::Char('W')));

    assert_eq!(saved(&actions), Some(WritingStyle::Plain));
    assert!(
        fixture
            .text()
            .contains("Writing style of the next round: plain")
    );
    assert_eq!(start_in_pane(&mut fixture).writing, WritingStyle::Plain);
}

#[test]
fn the_pane_shows_the_writing_style_as_saved() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);

    fixture
        .app
        .publish(ExploreRoundSettingsLoaded(ExploreRoundSettings {
            writing: WritingStyle::Plain,
            ..ExploreRoundSettings::default()
        }));
    let actions = fixture.click_actions(" Writing style of the next round: plain ");

    assert_eq!(
        saved(&actions),
        Some(WritingStyle::SimplifiedTechnicalEnglish)
    );
}

/// The run-ahead value that `actions` save, if they save one.
fn saved_run_ahead(actions: &[Action]) -> Option<RunAhead> {
    actions.iter().find_map(|action| match action {
        Action::Settings(SettingsAction::SaveExploreRunAhead(run_ahead)) => Some(*run_ahead),
        _ => None,
    })
}

#[test]
fn run_ahead_is_off_until_the_reviewer_turns_it_on_with_its_key() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);
    assert!(fixture.text().contains("Run ahead: off"));

    let actions = fixture.app.update(UserInput::Key(Key::Char('z')));

    assert_eq!(saved_run_ahead(&actions), Some(RunAhead::Recommended));
    assert!(fixture.text().contains("Run ahead: the recommended choice"));
    let actions = fixture.app.update(UserInput::Key(Key::Char('z')));
    assert_eq!(saved_run_ahead(&actions), Some(RunAhead::Every));
    assert!(fixture.text().contains("Run ahead: every choice"));
}

#[test]
fn the_pane_shows_run_ahead_as_saved_and_its_button_moves_it_on() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);

    fixture
        .app
        .publish(ExploreRoundSettingsLoaded(ExploreRoundSettings {
            run_ahead: RunAhead::Every,
            ..ExploreRoundSettings::default()
        }));
    let actions = fixture.click_actions(" Run ahead: every choice ");

    assert_eq!(saved_run_ahead(&actions), Some(RunAhead::Off));
}
