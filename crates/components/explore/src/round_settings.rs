//! The settings for the next Explore round, at the end of every screen of the pane, above the
//! Explore page's: the writing style of the texts the agent writes for the reviewer. A switch
//! changes at once and is saved alone with the reviewer's other settings; the pane then shows
//! the settings as saved. A round keeps the style it started with.

use review_explore_round_settings::{ExploreRoundSettings, WritingStyle};
use ui_actions::{Action, SettingsAction};
use ui_events::ExploreRoundSettingsLoaded;
use ui_shortcuts::ExploreSettingShortcut;
use ui_theme::Palette;

use super::flow::ConversationLayout;
use super::{Control, ExploreComponent};

/// The settings for the next round, as the pane shows and changes them.
#[derive(Default)]
pub(super) struct RoundSettings {
    saved: ExploreRoundSettings,
}

impl RoundSettings {
    /// The writing style a round started now takes.
    pub(super) fn writing(&self) -> WritingStyle {
        self.saved.writing
    }

    /// Turns the writing style over, shows it at once and saves it; the reviewer answers with
    /// the settings as saved.
    pub(super) fn toggle_writing(&mut self) -> Vec<Action> {
        self.saved.writing.toggle();
        vec![Action::Settings(SettingsAction::SaveExploreWritingStyle(
            self.saved.writing,
        ))]
    }

    /// The settings' controls, with their values.
    fn lay_out(&self, layout: &mut ConversationLayout, palette: Palette) {
        let style = match self.saved.writing {
            WritingStyle::Plain => "plain",
            WritingStyle::SimplifiedTechnicalEnglish => "Simplified Technical English",
        };
        layout.gap();
        layout.text("Explore round settings:", palette.dim, None);
        layout.controls([(
            format!("Writing style of the next round: {style}"),
            Control::Setting(ExploreSettingShortcut::WritingStyle),
        )]);
    }
}

impl ExploreComponent {
    /// Shows the settings for the next round as saved.
    pub(super) fn round_settings_loaded(&mut self, event: &ExploreRoundSettingsLoaded) {
        self.round_settings.saved = event.0;
    }

    /// The settings for the next round at the end of every screen of the pane, before the
    /// Explore page's.
    pub(super) fn lay_out_round_settings(&self, layout: &mut ConversationLayout, palette: Palette) {
        self.round_settings.lay_out(layout, palette);
    }
}
