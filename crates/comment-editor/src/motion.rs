//! Adapt direct vertical motions while leaving native Vim key sequences intact.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::{
    EditorEventHandler, EditorMode, EditorState, actions::SwitchMode, events::KeyEventRegister,
};

#[derive(Clone, Copy)]
pub(super) struct VisualMotion {
    pub(super) direction: isize,
    pub(super) step: VisualStep,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum VisualStep {
    Row,
    HalfPage,
    Page,
}

impl VisualStep {
    pub(super) fn distance(self, height: u16) -> usize {
        match self {
            Self::Row => 1,
            Self::HalfPage => usize::from((height / 2).max(1)),
            Self::Page => usize::from(height.max(1)),
        }
    }
}

impl VisualMotion {
    pub(super) fn resolve(
        event: KeyEvent,
        mode: EditorMode,
        handler: &EditorEventHandler,
    ) -> Option<Self> {
        if mode == EditorMode::Search {
            return None;
        }
        let motion = match (event.code, event.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) => Self {
                direction: -1,
                step: VisualStep::Row,
            },
            (KeyCode::Down, KeyModifiers::NONE) => Self {
                direction: 1,
                step: VisualStep::Row,
            },
            (KeyCode::PageUp, KeyModifiers::NONE) => Self {
                direction: -1,
                step: VisualStep::Page,
            },
            (KeyCode::PageDown, KeyModifiers::NONE) => Self {
                direction: 1,
                step: VisualStep::Page,
            },
            (KeyCode::Char('j'), KeyModifiers::NONE) if mode != EditorMode::Insert => Self {
                direction: 1,
                step: VisualStep::Row,
            },
            (KeyCode::Char('k'), KeyModifiers::NONE) if mode != EditorMode::Insert => Self {
                direction: -1,
                step: VisualStep::Row,
            },
            (KeyCode::Char('d'), KeyModifiers::CONTROL) if mode != EditorMode::Insert => Self {
                direction: 1,
                step: VisualStep::HalfPage,
            },
            (KeyCode::Char('u'), KeyModifiers::CONTROL) if mode != EditorMode::Insert => Self {
                direction: -1,
                step: VisualStep::HalfPage,
            },
            _ => return None,
        };
        // Edtui keeps pending sequences private and accepts only its own action
        // enum. Probe a cloned handler with a marker action: `fj`, `dk`, searches,
        // etc. still use Edtui's parser and cannot be mistaken for a bare motion.
        let mut probe = handler.clone();
        probe.key_handler.insert(
            KeyEventRegister::new(vec![edtui::events::KeyInput::from(event)], mode),
            SwitchMode(EditorMode::Search),
        );
        let mut marker = EditorState::default();
        marker.mode = mode;
        probe.on_key_event(event, &mut marker);
        (marker.mode == EditorMode::Search).then_some(motion)
    }
}
