//! Normalized keyboard input and its one mapping to and from terminal key events.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// One normalized keyboard input.
///
/// Every variant is produced by [`Key::from_terminal`], and
/// [`Key::to_terminal`] gives back the terminal event it came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Char(char),
    Control(char),
    Alt(char),
    Backspace,
    Delete,
    Tab,
    Left,
    Right,
    Down,
    Up,
    PageDown,
    PageUp,
    First,
    Last,
    HalfPageDown,
    HalfPageUp,
    PreviousLocation,
    NextLocation,
    Escape,
    Enter,
    ControlEnter,
    EditorMode,
}

impl Key {
    /// Normalize a terminal key event, or return `None` for events the UI
    /// does not handle, such as key releases.
    pub fn from_terminal(event: KeyEvent) -> Option<Self> {
        if event.kind == KeyEventKind::Release {
            return None;
        }
        if event.modifiers.contains(KeyModifiers::CONTROL) {
            return NamedKey::find(event.code, KeyModifiers::CONTROL)
                .map(|named| named.key)
                .or(match event.code {
                    KeyCode::Char(character) => Some(Self::Control(character)),
                    _ => None,
                });
        }
        match event.code {
            KeyCode::Char(character) if event.modifiers.contains(KeyModifiers::ALT) => {
                Some(Self::Alt(character))
            }
            KeyCode::Char(character) => Some(Self::Char(character)),
            code => NamedKey::find(code, KeyModifiers::NONE).map(|named| named.key),
        }
    }

    /// The terminal key event that normalizes to this key.
    pub fn to_terminal(self) -> KeyEvent {
        let (code, modifiers) = match self {
            Self::Char(character) => (KeyCode::Char(character), KeyModifiers::NONE),
            Self::Control(character) => (KeyCode::Char(character), KeyModifiers::CONTROL),
            Self::Alt(character) => (KeyCode::Char(character), KeyModifiers::ALT),
            _ => {
                let named = NamedKey::of(self);
                (named.code, named.modifiers)
            }
        };
        KeyEvent::new(code, modifiers)
    }

    /// How help text names this key.
    pub fn label(self) -> String {
        match self {
            Self::Char(' ') => "Space".to_owned(),
            Self::Char(character) => character.to_string(),
            Self::Control(character) => format!("Ctrl-{character}"),
            Self::Alt(character) => format!("Alt-{character}"),
            _ => NamedKey::of(self).label.to_owned(),
        }
    }
}

/// A key that is not a plain, Control or Alt character: the terminal event
/// that produces it and the label that shows it in help.
struct NamedKey {
    key: Key,
    code: KeyCode,
    modifiers: KeyModifiers,
    label: &'static str,
}

impl NamedKey {
    const fn new(key: Key, code: KeyCode, modifiers: KeyModifiers, label: &'static str) -> Self {
        Self {
            key,
            code,
            modifiers,
            label,
        }
    }

    fn of(key: Key) -> &'static Self {
        NAMED_KEYS
            .iter()
            .find(|named| named.key == key)
            .expect("each non-character key must have a terminal event")
    }

    fn find(code: KeyCode, modifiers: KeyModifiers) -> Option<&'static Self> {
        NAMED_KEYS
            .iter()
            .find(|named| named.code == code && named.modifiers == modifiers)
    }
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CONTROL: KeyModifiers = KeyModifiers::CONTROL;

const NAMED_KEYS: &[NamedKey] = &[
    NamedKey::new(Key::Backspace, KeyCode::Backspace, NONE, "Backspace"),
    NamedKey::new(Key::Delete, KeyCode::Delete, NONE, "Delete"),
    NamedKey::new(Key::Tab, KeyCode::Tab, NONE, "Tab"),
    NamedKey::new(Key::Left, KeyCode::Left, NONE, "Left"),
    NamedKey::new(Key::Right, KeyCode::Right, NONE, "Right"),
    NamedKey::new(Key::Down, KeyCode::Down, NONE, "Down"),
    NamedKey::new(Key::Up, KeyCode::Up, NONE, "Up"),
    NamedKey::new(Key::PageDown, KeyCode::PageDown, NONE, "PageDown"),
    NamedKey::new(Key::PageUp, KeyCode::PageUp, NONE, "PageUp"),
    NamedKey::new(Key::First, KeyCode::Home, NONE, "Home"),
    NamedKey::new(Key::Last, KeyCode::End, NONE, "End"),
    NamedKey::new(Key::HalfPageDown, KeyCode::Char('d'), CONTROL, "Ctrl-d"),
    NamedKey::new(Key::HalfPageUp, KeyCode::Char('u'), CONTROL, "Ctrl-u"),
    NamedKey::new(Key::PreviousLocation, KeyCode::Char('o'), CONTROL, "Ctrl-o"),
    NamedKey::new(Key::NextLocation, KeyCode::Char('i'), CONTROL, "Ctrl-i"),
    NamedKey::new(Key::Escape, KeyCode::Esc, NONE, "Esc"),
    NamedKey::new(Key::Enter, KeyCode::Enter, NONE, "Enter"),
    NamedKey::new(Key::ControlEnter, KeyCode::Enter, CONTROL, "Ctrl-Enter"),
    NamedKey::new(Key::EditorMode, KeyCode::F(2), NONE, "F2"),
];

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
