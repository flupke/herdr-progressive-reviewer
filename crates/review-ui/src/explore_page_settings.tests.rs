//! The settings of the Explore page in the Explore tab: shown with their values on every screen,
//! changed in place, and saved one at a time with the reviewer's other settings.

use super::*;
use crate::ExplorePageAction;
use review_explore_page_settings::{ExplorePageSetting, ExplorePageSettings, PaneStarts};
use ui_events::{ExplorePageOffNetwork, ExplorePageSettingsLoaded, ExplorePageShared};

const URL: &str = "http://192.168.1.23:8790/?token=0123456789abcdef0123456789abcdef";

/// The Explore page setting that `actions` save, if they save one.
fn saved(actions: &[Action]) -> Option<ExplorePageSetting> {
    actions.iter().find_map(|action| match action {
        Action::Settings(SettingsAction::SaveExplorePage(setting)) => Some(setting.clone()),
        _ => None,
    })
}

impl ExploreUi {
    fn press(&mut self, key: Key) -> Vec<Action> {
        self.app.update(UserInput::Key(key))
    }

    fn type_text(&mut self, text: &str) -> Vec<Action> {
        text.chars()
            .flat_map(|character| self.press(Key::Char(character)))
            .collect()
    }
}

#[test]
fn the_start_screen_shows_the_settings_of_the_explore_page_with_their_defaults() {
    let fixture = ExploreUi::start_screen(BASE, POLICY);

    let text = fixture.text();
    for label in [
        "Open the page on Start: on",
        "Serve on the network: on",
        "Interface: default route",
        "First port: 8790",
    ] {
        assert!(text.contains(label), "{label}: {text}");
    }
}

#[test]
fn the_start_screen_shows_the_settings_the_reviewer_read_when_it_started() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);
    let mut settings = ExplorePageSettings {
        pane_starts: PaneStarts::InPane,
        ..ExplorePageSettings::default()
    };
    settings.network.enabled = false;
    settings.network.set_interface("tailscale0");
    settings.set(ExplorePageSetting::first_port("9100").unwrap());

    fixture.app.publish(ExplorePageSettingsLoaded(settings));

    let text = fixture.text();
    for label in [
        "Open the page on Start: off",
        "Serve on the network: off",
        "Interface: tailscale0",
        "First port: 9100",
    ] {
        assert!(text.contains(label), "{label}: {text}");
    }
}

#[test]
fn turning_the_opening_off_saves_it_and_start_then_starts_the_round_in_the_pane() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);

    let actions = fixture.press(Key::Char('w'));

    assert_eq!(
        saved(&actions),
        Some(ExplorePageSetting::PaneStarts(PaneStarts::InPane))
    );
    assert!(fixture.text().contains("Open the page on Start: off"));
    fixture.press(Key::Char('s'));
    let captured = fixture.app.publish(ExploreCaptured {
        result: Ok(fixture.comparison.clone()),
    });
    assert!(!captured.contains(&Action::ExplorePage(ExplorePageAction::Open)));
    let request = ExploreUi::request(captured);
    fixture.respond(&request, 1);
    assert!(fixture.text().contains("Question 1"));
}

#[test]
fn turning_network_access_off_saves_it_and_the_pane_drops_the_address_once_the_page_is_off() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);
    fixture.app.publish(ExplorePageShared(URL.into()));

    let actions = fixture.click_actions(" Serve on the network: on ");

    assert_eq!(
        saved(&actions),
        Some(ExplorePageSetting::NetworkEnabled(false))
    );
    fixture.app.publish(ExplorePageOffNetwork);
    let text = fixture.text();
    assert!(!text.contains(URL), "{text}");
    assert!(text.contains("Serve on the network: off"), "{text}");

    let actions = fixture.press(Key::Char('n'));
    assert_eq!(
        saved(&actions),
        Some(ExplorePageSetting::NetworkEnabled(true))
    );
    fixture.app.publish(ExplorePageShared(URL.into()));
    assert!(fixture.text().contains(URL));
}

#[test]
fn the_interface_is_typed_in_an_editor_and_saved_with_enter() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);

    fixture.press(Key::Char('N'));
    let typed = fixture.type_text("tailscale0 qs");
    assert!(typed.is_empty(), "typing runs no command: {typed:?}");
    for _ in 0..3 {
        fixture.press(Key::Backspace);
    }
    let actions = fixture.press(Key::Enter);

    assert_eq!(
        saved(&actions),
        Some(ExplorePageSetting::Interface(Some("tailscale0".into())))
    );
    let text = fixture.text();
    assert!(text.contains("Interface: tailscale0"), "{text}");
}

#[test]
fn an_empty_interface_saves_the_interface_of_the_default_route() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);
    let mut settings = ExplorePageSettings::default();
    settings.network.set_interface("wlan0");
    fixture.app.publish(ExplorePageSettingsLoaded(settings));

    fixture.click(" Interface: wlan0 ");
    for _ in 0.."wlan0".len() {
        fixture.press(Key::Backspace);
    }
    let actions = fixture.press(Key::Enter);

    assert_eq!(saved(&actions), Some(ExplorePageSetting::Interface(None)));
    assert!(fixture.text().contains("Interface: default route"));
}

#[test]
fn a_first_port_that_is_not_a_port_says_why_and_saves_nothing_until_it_is_corrected() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);

    fixture.press(Key::Char('#'));
    fixture.type_text("x");
    let refused = fixture.press(Key::Enter);

    assert_eq!(saved(&refused), None);
    let text = fixture.text();
    assert!(text.contains("8790x"), "the editor keeps the text: {text}");
    assert!(text.contains("from 1 to 65535"), "{text}");
    for _ in 0.."8790x".len() {
        fixture.press(Key::Backspace);
    }
    fixture.type_text("9000");
    let actions = fixture.press(Key::Enter);
    assert_eq!(saved(&actions), Some(ExplorePageSetting::FirstPort(9000)));
    let text = fixture.text();
    assert!(text.contains("First port: 9000"), "{text}");
    assert!(!text.contains("from 1 to 65535"), "{text}");
}

#[test]
fn while_a_setting_is_typed_the_footer_offers_no_start_keys() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);
    assert!(fixture.footer().contains("s start"));

    fixture.press(Key::Char('N'));

    assert!(
        !fixture.footer().contains("s start"),
        "{}",
        fixture.footer()
    );
    fixture.press(Key::Tab);
    assert!(fixture.footer().contains("s start"));
}

#[test]
fn tab_leaves_a_field_without_saving_it() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);

    fixture.press(Key::Char('#'));
    fixture.type_text("1");
    let actions = fixture.press(Key::Tab);

    assert_eq!(saved(&actions), None);
    let text = fixture.text();
    assert!(!text.contains("87901"), "{text}");
    assert!(text.contains("First port: 8790"), "{text}");
    assert_eq!(
        saved(&fixture.press(Key::Char('w'))),
        Some(ExplorePageSetting::PaneStarts(PaneStarts::InPane)),
        "the keys of the start screen work again"
    );
}

#[test]
fn the_settings_saved_since_show_in_place_of_the_ones_the_pane_had() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);
    fixture.press(Key::Char('#'));
    let mut saved = ExplorePageSettings::default();
    saved.network.enabled = false;

    fixture.app.publish(ExplorePageSettingsLoaded(saved));

    let text = fixture.text();
    assert!(text.contains("Serve on the network: off"), "{text}");
    assert!(
        text.contains("First port · Enter save"),
        "the setting being typed stays: {text}"
    );
}

#[test]
fn a_round_in_the_pane_shows_the_settings_and_turns_network_access_off() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.publish(ExplorePageShared(URL.into()));
    // The settings come at the end of the round's page.
    fixture.app.update(UserInput::MouseScroll {
        column: 0,
        row: 4,
        delta: 500,
    });

    let actions = fixture.click_actions(" Serve on the network: on ");

    assert_eq!(
        saved(&actions),
        Some(ExplorePageSetting::NetworkEnabled(false))
    );
    fixture.app.publish(ExplorePageOffNetwork);
    assert!(!fixture.text().contains(URL));
}

#[test]
fn a_round_on_the_page_takes_the_keys_of_the_settings() {
    let (mut fixture, _) = ExploreUi::started(BASE, POLICY, 's');
    assert!(fixture.text().contains("runs on the Explore page"));

    let actions = fixture.press(Key::Char('n'));

    assert_eq!(
        saved(&actions),
        Some(ExplorePageSetting::NetworkEnabled(false))
    );
    fixture.press(Key::Char('N'));
    let typed = fixture.type_text("io");
    assert!(typed.is_empty(), "typing opens nothing: {typed:?}");
    assert!(fixture.text().contains("Network interface · empty"));
}

#[test]
fn another_control_leaves_the_setting_being_typed_unsaved() {
    let mut fixture = ExploreUi::start_screen(BASE, POLICY);
    fixture.press(Key::Char('#'));
    fixture.type_text("1");

    let actions = fixture.click_actions(" Start in the pane ");

    assert_eq!(saved(&actions), None);
    assert!(!fixture.text().contains("First port · Enter save"));
}
