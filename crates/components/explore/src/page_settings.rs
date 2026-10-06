//! The settings of the Explore page, at the end of every screen of the pane: whether Start and
//! Start with Challenger open the page, and whether, and where, the page is served to the
//! network. The two switches change at once; the interface and the first port are typed in the
//! shared text editor and saved with Enter. Each change is saved alone with the reviewer's other
//! settings, the reviewer applies it at once, and the pane then shows the settings as saved.
//! With them, a switch that shares the running round over a tunnel, which is not saved: it
//! starts off, and goes off with the round.

use comment_editor::{CommentEditor, KeymapSetting};
use review_explore_page_settings::{
    ExplorePageSetting, ExplorePageSettings, NetworkAccess, PaneStarts,
};
use review_explore_page_tunnel::TunnelState;
use ui_actions::{Action, ExplorePageAction, SettingsAction};
use ui_events::{ExplorePageSettingsLoaded, ExplorePageTunnel};
use ui_shortcuts::{
    ExploreCommand, ExploreGlobalShortcut, ExplorePageSettingShortcut, ExploreSettingShortcut,
    ExploreShortcut,
};
use ui_theme::Palette;

use super::flow::{Content, ConversationLayout};
use super::input::ExploreKey;
use super::network_page::SharedRound;
use super::{Control, ExploreComponent};

/// The settings of the Explore page, as the pane shows and changes them.
#[derive(Default)]
pub(super) struct PageSettings {
    saved: ExplorePageSettings,
    /// The setting being typed, if any.
    field: Option<FieldEdit>,
    /// The tunnel that shares the running round, as the page host last reported it.
    tunnel: SharedRound,
}

/// A setting typed in a text editor, until Enter saves it or Tab leaves it.
struct FieldEdit {
    field: TextField,
    editor: CommentEditor,
    /// Why Enter did not save the text.
    error: Option<String>,
}

/// A setting the reviewer types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextField {
    Interface,
    FirstPort,
}

impl TextField {
    /// The editor's title, with its keys.
    fn title(self) -> &'static str {
        match self {
            Self::Interface => {
                "Network interface · empty for the default route · Enter save · Tab cancel"
            }
            Self::FirstPort => "First port · Enter save · Tab cancel",
        }
    }

    /// The setting's value in `network`, as the editor opens with it.
    fn text(self, network: &NetworkAccess) -> String {
        match self {
            Self::Interface => network.interface.clone().unwrap_or_default(),
            Self::FirstPort => network.first_port.to_string(),
        }
    }

    /// The setting with the typed `text` as its value, or why `text` cannot be one.
    fn setting(self, text: &str) -> Result<ExplorePageSetting, String> {
        match self {
            Self::Interface => Ok(ExplorePageSetting::interface(text)),
            Self::FirstPort => ExplorePageSetting::first_port(text),
        }
    }
}

impl PageSettings {
    /// What Start and Start with Challenger do in the pane.
    pub(super) fn pane_starts(&self) -> PaneStarts {
        self.saved.pane_starts
    }

    /// The tunnel that shares the running round.
    pub(super) fn tunnel(&self) -> &SharedRound {
        &self.tunnel
    }

    /// Whether a setting is being typed: its editor takes every key.
    pub(super) fn editing(&self) -> bool {
        self.field.is_some()
    }

    /// The text editor of the setting being typed.
    pub(super) fn editor(&self) -> Option<(&CommentEditor, &'static str)> {
        self.field
            .as_ref()
            .map(|field| (&field.editor, field.field.title()))
    }

    /// Runs `setting`'s control: turns a switch over and saves it, or opens a field's editor
    /// with the setting's text.
    pub(super) fn change(
        &mut self,
        setting: ExplorePageSettingShortcut,
        keymap: &KeymapSetting,
    ) -> Vec<Action> {
        let field = match setting {
            ExplorePageSettingShortcut::PaneStarts => {
                let mut starts = self.saved.pane_starts;
                starts.toggle();
                return self.save(ExplorePageSetting::PaneStarts(starts));
            }
            ExplorePageSettingShortcut::Network => {
                let enabled = !self.saved.network.enabled;
                return self.save(ExplorePageSetting::NetworkEnabled(enabled));
            }
            ExplorePageSettingShortcut::Tunnel => return self.toggle_tunnel(),
            ExplorePageSettingShortcut::Interface => TextField::Interface,
            ExplorePageSettingShortcut::FirstPort => TextField::FirstPort,
        };
        self.field = Some(FieldEdit {
            field,
            editor: CommentEditor::single_line(&field.text(&self.saved.network), keymap),
            error: None,
        });
        Vec::new()
    }

    /// Stops the tunnel that runs or opens, or opens one for the running round. Shows the change
    /// at once; the page host then reports where the tunnel stands.
    fn toggle_tunnel(&mut self) -> Vec<Action> {
        let (state, action) = if self.tunnel.state().is_on() {
            (TunnelState::Off, ExplorePageAction::CloseTunnel)
        } else {
            (TunnelState::Opening, ExplorePageAction::OpenTunnel)
        };
        self.tunnel = SharedRound::new(state);
        vec![Action::ExplorePage(action)]
    }

    /// Saves the typed setting, or keeps its editor open with the reason it cannot.
    fn save_field(&mut self) -> Vec<Action> {
        let Some(edit) = &mut self.field else {
            return Vec::new();
        };
        match edit.field.setting(&edit.editor.text()) {
            Ok(setting) => {
                self.field = None;
                self.save(setting)
            }
            Err(error) => {
                edit.error = Some(error);
                Vec::new()
            }
        }
    }

    /// Shows `setting` at once and saves it; the reviewer answers with the settings as saved.
    fn save(&mut self, setting: ExplorePageSetting) -> Vec<Action> {
        self.saved.set(setting.clone());
        vec![Action::Settings(SettingsAction::SaveExplorePage(setting))]
    }

    /// Leaves the setting being typed, if any, unsaved.
    fn close_field(&mut self) {
        self.field = None;
    }

    /// Runs `input` on the setting being typed: Enter saves it, Tab leaves it, and every other
    /// key goes to its editor. `None` when no setting is typed.
    fn key(&mut self, input: ExploreKey) -> Option<Vec<Action>> {
        let edit = self.field.as_mut()?;
        match input.command {
            Some(ExploreCommand::Explore(ExploreShortcut::Confirm)) => {
                return Some(self.save_field());
            }
            Some(ExploreCommand::Global(ExploreGlobalShortcut::CycleFocus)) => self.close_field(),
            _ => edit.editor.input(input.key),
        }
        Some(Vec::new())
    }

    /// Pastes `text` into the setting being typed. Returns whether one is.
    fn paste(&mut self, text: &str) -> bool {
        let Some(edit) = &mut self.field else {
            return false;
        };
        edit.editor.paste(text);
        true
    }

    /// The settings' controls, with their values, then the editor of the setting being typed
    /// and why it was not saved.
    fn lay_out(&self, layout: &mut ConversationLayout, palette: Palette) {
        let network = &self.saved.network;
        let on = |value| if value { "on" } else { "off" };
        layout.gap();
        layout.text("Explore page settings:", palette.dim, None);
        layout.controls([
            (
                format!(
                    "Open the page on Start: {}",
                    on(self.saved.pane_starts == PaneStarts::OnPage)
                ),
                Control::Setting(ExploreSettingShortcut::Page(
                    ExplorePageSettingShortcut::PaneStarts,
                )),
            ),
            (
                format!("Serve on the network: {}", on(network.enabled)),
                Control::Setting(ExploreSettingShortcut::Page(
                    ExplorePageSettingShortcut::Network,
                )),
            ),
            (
                format!(
                    "Interface: {}",
                    network.interface.as_deref().unwrap_or("default route")
                ),
                Control::Setting(ExploreSettingShortcut::Page(
                    ExplorePageSettingShortcut::Interface,
                )),
            ),
            (
                format!("First port: {}", network.first_port),
                Control::Setting(ExploreSettingShortcut::Page(
                    ExplorePageSettingShortcut::FirstPort,
                )),
            ),
            (
                format!(
                    "Share over a tunnel: {}",
                    match self.tunnel.state() {
                        TunnelState::Off | TunnelState::Failed(_) => "off",
                        TunnelState::Opening => "opening",
                        TunnelState::Open { .. } => "on",
                    }
                ),
                Control::Setting(ExploreSettingShortcut::Page(
                    ExplorePageSettingShortcut::Tunnel,
                )),
            ),
        ]);
        let Some(edit) = &self.field else {
            return;
        };
        layout.push(Content::SettingEditor, 3);
        if let Some(error) = &edit.error {
            layout.text(error.clone(), palette.warning, None);
        }
    }
}

impl ExploreComponent {
    /// Shows the settings as saved, keeping the setting being typed.
    pub(super) fn page_settings_loaded(&mut self, event: &ExplorePageSettingsLoaded) {
        self.page_settings.saved = event.0.clone();
    }

    /// Shows where the tunnel that shares the running round stands.
    pub(super) fn tunnel_reported(&mut self, event: &ExplorePageTunnel) {
        self.page_settings.tunnel = SharedRound::new(event.0.clone());
    }

    /// Leaves the setting being typed unsaved: another control of the pane was used.
    pub(super) fn leave_page_setting(&mut self) {
        self.page_settings.close_field();
    }

    /// A key while a setting is typed: Enter saves it, Tab leaves it, and every other key goes
    /// to its editor. `None` when no setting is typed.
    pub(super) fn page_setting_key(&mut self, input: ExploreKey) -> Option<Vec<Action>> {
        self.page_settings.key(input)
    }

    /// Pastes `text` into the setting being typed. Returns whether one is.
    pub(super) fn paste_page_setting(&mut self, text: &str) -> bool {
        self.page_settings.paste(text)
    }

    /// The settings at the end of every screen of the pane, before the page's address.
    pub(super) fn lay_out_page_settings(&self, layout: &mut ConversationLayout, palette: Palette) {
        self.page_settings.lay_out(layout, palette);
    }
}
