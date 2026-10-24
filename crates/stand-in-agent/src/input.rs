//! The stand-in agent's input box, as a terminal agent's composer reads what is typed in it.

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

#[derive(Default)]
pub(crate) struct AgentInput {
    text: String,
}

impl AgentInput {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Puts the prompt `blocked`, if any, back in the composer, as Claude Code does with a
    /// prompt its hooks blocked.
    pub(crate) fn restore(&mut self, blocked: Option<String>) {
        if let Some(blocked) = blocked {
            self.text = blocked;
        }
    }

    pub(crate) fn handle(&mut self, event: Event) -> Option<String> {
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
