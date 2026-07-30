//! UI palettes and their paired syntax themes.

#![allow(clippy::unreadable_literal)]

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use ratatui::style::Color;
use two_face::theme::EmbeddedThemeName;

const DEFAULT: &str = "catppuccin";

/// One complete UI and syntax theme.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Theme {
    pub(crate) palette: Palette,
    pub(crate) syntax: EmbeddedThemeName,
}

/// The colors that the review UI uses.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Palette {
    pub(crate) text: Color,
    pub(crate) dim: Color,
    pub(crate) focus: Color,
    pub(crate) warning: Color,
    pub(crate) deletion: Color,
    pub(crate) insertion: Color,
    pub(crate) selection: Color,
    pub(crate) cursor: Color,
    pub(crate) deletion_bg: Color,
    pub(crate) insertion_bg: Color,
}

#[derive(Clone, Copy)]
enum Appearance {
    Dark,
    Light,
}

#[derive(Clone, Copy)]
struct Anchors {
    base: Color,
    text: Color,
    red: Color,
    green: Color,
    yellow: Color,
    focus: Color,
}

impl Theme {
    /// Read the optional theme from the Herdr plugin configuration.
    pub(crate) fn from_env() -> eyre::Result<Self> {
        let Some(directory) = env::var_os("HERDR_PLUGIN_CONFIG_DIR") else {
            return Ok(Self::default());
        };
        Self::from_config_dir(&PathBuf::from(directory))
    }

    fn from_config_dir(directory: &Path) -> eyre::Result<Self> {
        let path = directory.join("config.toml");
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.into()),
        };
        let config: toml::Value = toml::from_str(&source)?;
        let Some(value) = config.get("theme") else {
            return Ok(Self::default());
        };
        let name = value
            .as_str()
            .ok_or_else(|| eyre::eyre!("`theme` in {} must be a string", path.display()))?;
        Self::resolve(name)
            .ok_or_else(|| eyre::eyre!("unknown theme {name:?} in {}", path.display()))
    }

    fn resolve(name: &str) -> Option<Self> {
        use Appearance::{Dark, Light};
        use EmbeddedThemeName as Syntax;

        let (appearance, syntax, anchors) = match name {
            "catppuccin" => (Dark, Syntax::CatppuccinMocha, CATPPUCCIN),
            "catppuccin-latte" => (Light, Syntax::CatppuccinLatte, CATPPUCCIN_LATTE),
            "catppuccin-frappe" => (Dark, Syntax::CatppuccinFrappe, CATPPUCCIN_FRAPPE),
            "catppuccin-macchiato" => (Dark, Syntax::CatppuccinMacchiato, CATPPUCCIN_MACCHIATO),
            "dracula" => (Dark, Syntax::Dracula, DRACULA),
            "nord" => (Dark, Syntax::Nord, NORD),
            "gruvbox" => (Dark, Syntax::GruvboxDark, GRUVBOX),
            "gruvbox-light" => (Light, Syntax::GruvboxLight, GRUVBOX_LIGHT),
            "one-dark" => (Dark, Syntax::TwoDark, ONE_DARK),
            "one-light" => (Light, Syntax::OneHalfLight, ONE_LIGHT),
            "solarized" => (Dark, Syntax::SolarizedDark, SOLARIZED),
            "solarized-light" => (Light, Syntax::SolarizedLight, SOLARIZED_LIGHT),
            "github-light" => (Light, Syntax::Github, GITHUB_LIGHT),
            "monokai" => (Dark, Syntax::MonokaiExtended, MONOKAI),
            _ => return None,
        };
        Some(Self {
            palette: Palette::from_anchors(anchors, appearance),
            syntax,
        })
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::resolve(DEFAULT).expect("the default theme is valid")
    }
}

impl Palette {
    fn from_anchors(anchors: Anchors, appearance: Appearance) -> Self {
        let (pole, tint) = match appearance {
            Appearance::Dark => (Anchors::color(0xff_ff_ff), 20),
            Appearance::Light => (Anchors::color(0x00_00_00), 12),
        };
        Self {
            text: anchors.text,
            dim: Self::blend(anchors.base, pole, 34),
            focus: anchors.focus,
            warning: anchors.yellow,
            deletion: anchors.red,
            insertion: anchors.green,
            selection: Self::blend(anchors.base, pole, 9),
            cursor: Self::blend(anchors.base, pole, 14),
            deletion_bg: Self::blend(anchors.base, anchors.red, tint),
            insertion_bg: Self::blend(anchors.base, anchors.green, tint),
        }
    }

    fn blend(from: Color, to: Color, percent: u16) -> Color {
        let (Color::Rgb(fr, fg, fb), Color::Rgb(tr, tg, tb)) = (from, to) else {
            return from;
        };
        let mix = |left: u8, right: u8| {
            let value = (u16::from(left) * (100 - percent) + u16::from(right) * percent + 50) / 100;
            u8::try_from(value).expect("a blend of two bytes is a byte")
        };
        Color::Rgb(mix(fr, tr), mix(fg, tg), mix(fb, tb))
    }
}

impl Anchors {
    const fn new(base: u32, text: u32, red: u32, green: u32, yellow: u32, focus: u32) -> Self {
        Self {
            base: Self::color(base),
            text: Self::color(text),
            red: Self::color(red),
            green: Self::color(green),
            yellow: Self::color(yellow),
            focus: Self::color(focus),
        }
    }

    const fn color(rgb: u32) -> Color {
        let [_, red, green, blue] = rgb.to_be_bytes();
        Color::Rgb(red, green, blue)
    }
}

const CATPPUCCIN: Anchors =
    Anchors::new(0x1e1e2e, 0xcdd6f4, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0xb4befe);
const CATPPUCCIN_LATTE: Anchors =
    Anchors::new(0xeff1f5, 0x4c4f69, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x7287fd);
const CATPPUCCIN_FRAPPE: Anchors =
    Anchors::new(0x303446, 0xc6d0f5, 0xe78284, 0xa6d189, 0xe5c890, 0xbabbf1);
const CATPPUCCIN_MACCHIATO: Anchors =
    Anchors::new(0x24273a, 0xcad3f5, 0xed8796, 0xa6da95, 0xeed49f, 0xb7bdf8);
const DRACULA: Anchors = Anchors::new(0x282a36, 0xf8f8f2, 0xff5555, 0x50fa7b, 0xf1fa8c, 0x8be9fd);
const NORD: Anchors = Anchors::new(0x2e3440, 0xd8dee9, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1);
const GRUVBOX: Anchors = Anchors::new(0x282828, 0xebdbb2, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598);
const GRUVBOX_LIGHT: Anchors =
    Anchors::new(0xfbf1c7, 0x3c3836, 0x9d0006, 0x79740e, 0xb57614, 0x076678);
const ONE_DARK: Anchors = Anchors::new(0x282c34, 0xabb2bf, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef);
const ONE_LIGHT: Anchors = Anchors::new(0xfafafa, 0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2);
const SOLARIZED: Anchors = Anchors::new(0x002b36, 0x93a1a1, 0xdc322f, 0x859900, 0xb58900, 0x268bd2);
const SOLARIZED_LIGHT: Anchors =
    Anchors::new(0xfdf6e3, 0x586e75, 0xdc322f, 0x859900, 0xb58900, 0x268bd2);
const GITHUB_LIGHT: Anchors =
    Anchors::new(0xffffff, 0x1f2328, 0xcf222e, 0x1a7f37, 0x9a6700, 0x0969da);
const MONOKAI: Anchors = Anchors::new(0x272822, 0xf8f8f2, 0xf92672, 0xa6e22e, 0xe6db74, 0x66d9ef);

#[cfg(test)]
mod tests {
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
}
