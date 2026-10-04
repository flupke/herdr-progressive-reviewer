use super::*;

#[test]
fn the_defaults_open_the_page_and_serve_it_on_the_interface_of_the_route_to_the_internet() {
    let settings = ExplorePageSettings::default();

    assert_eq!(settings.pane_starts, PaneStarts::OnPage);
    assert!(settings.network.enabled);
    assert_eq!(settings.network.interface, None);
    assert_eq!(settings.network.first_port, 8790);
}

#[test]
fn settings_saved_without_a_value_take_its_default() {
    let empty: ExplorePageSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, ExplorePageSettings::default());

    let partial: ExplorePageSettings =
        serde_json::from_str(r#"{"network": {"enabled": false}}"#).unwrap();
    assert!(!partial.network.enabled);
    assert_eq!(partial.network.first_port, 8790);
    assert_eq!(partial.pane_starts, PaneStarts::OnPage);
}

#[test]
fn settings_read_back_as_they_were_saved() {
    let settings = ExplorePageSettings {
        pane_starts: PaneStarts::InPane,
        network: NetworkAccess {
            enabled: false,
            interface: Some("tailscale0".into()),
            first_port: 9000,
        },
    };

    let saved = serde_json::to_string(&settings).unwrap();

    assert_eq!(
        serde_json::from_str::<ExplorePageSettings>(&saved).unwrap(),
        settings
    );
}

#[test]
fn turning_a_setting_over_gives_the_other_value() {
    let mut starts = PaneStarts::OnPage;

    starts.toggle();
    assert_eq!(starts, PaneStarts::InPane);
    starts.toggle();
    assert_eq!(starts, PaneStarts::OnPage);
}

#[test]
fn an_empty_interface_name_means_the_interface_of_the_route_to_the_internet() {
    let mut network = NetworkAccess::default();

    network.set_interface("  wlan0 ");
    assert_eq!(network.interface.as_deref(), Some("wlan0"));

    network.set_interface("   ");
    assert_eq!(network.interface, None);
}

#[test]
fn the_first_port_is_a_number_from_1_to_65535() {
    assert_eq!(
        ExplorePageSetting::first_port(" 8800 "),
        Ok(ExplorePageSetting::FirstPort(8800))
    );
    for refused in ["0", "65536", "", "eighty", "-1"] {
        assert!(
            ExplorePageSetting::first_port(refused).is_err(),
            "{refused:?}"
        );
    }
}

#[test]
fn a_setting_changes_its_value_and_keeps_the_others() {
    let mut settings = ExplorePageSettings::default();
    settings.network.set_interface("wlan0");

    settings.set(ExplorePageSetting::FirstPort(9000));
    settings.set(ExplorePageSetting::NetworkEnabled(false));

    assert_eq!(settings.network.first_port, 9000);
    assert!(!settings.network.enabled);
    assert_eq!(settings.network.interface.as_deref(), Some("wlan0"));
    assert_eq!(settings.pane_starts, PaneStarts::OnPage);
    settings.set(ExplorePageSetting::Interface(None));
    settings.set(ExplorePageSetting::PaneStarts(PaneStarts::InPane));
    assert_eq!(settings.network.interface, None);
    assert_eq!(settings.pane_starts, PaneStarts::InPane);
}
