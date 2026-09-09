//! A terminal composer for the deterministic agent used by isolated Herdr tests.

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

#[derive(Default)]
pub(super) struct AgentInput {
    text: String,
}

impl AgentInput {
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn handle(&mut self, event: Event) -> Option<String> {
        match event {
            Event::Paste(text) => self.text.push_str(&text),
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                match (key.code, key.modifiers) {
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        return Some(std::mem::take(&mut self.text));
                    }
                    (KeyCode::Char('u' | 'c'), KeyModifiers::CONTROL) => self.text.clear(),
                    (KeyCode::Char(character), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                        self.text.push(character);
                    }
                    (KeyCode::Backspace, _) => {
                        self.text.pop();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        None
    }
}
