use super::Theme;

#[test]
fn resolves_dark_and_light_palettes() {
    assert!(Theme::resolve("catppuccin").is_some());
    assert!(Theme::resolve("catppuccin-latte").is_some());
    assert!(Theme::resolve("unknown").is_none());
}

#[test]
fn missing_plugin_config_uses_the_default() {
    // The test process normally has no Herdr plugin environment.
    if std::env::var_os("HERDR_PLUGIN_CONFIG_DIR").is_none() {
        assert_eq!(
            Theme::from_env().unwrap().palette.text,
            Theme::resolve("catppuccin").unwrap().palette.text
        );
    }
}

#[test]
fn reads_the_palette_from_plugin_config() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("config.toml"),
        "theme = \"gruvbox\"\n",
    )
    .unwrap();

    assert_eq!(
        Theme::from_config_dir(directory.path())
            .unwrap()
            .palette
            .text,
        Theme::resolve("gruvbox").unwrap().palette.text
    );
}
