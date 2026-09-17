use super::{Control, ExploreComponent, Reveal};
use ui_shortcuts::Key;

impl ExploreComponent {
    pub(super) fn select_choice(&mut self, choice: usize) {
        if self.selected_choice().is_some()
            && self
                .question()
                .is_some_and(|question| question.choices().nth(choice).is_some())
        {
            self.turns[self.selected].choice = choice;
            self.editing = false;
            self.reveal.set(Some(Reveal::Choice));
        }
    }

    pub(super) fn selected_choice(&self) -> Option<usize> {
        if !self.progress.can_submit() {
            return None;
        }
        let question = self.question()?;
        let exploration = self.exploration.as_ref()?;
        if self.correction.is_some()
            || exploration.conclusion.is_some()
            || !exploration.can_choose(question)
        {
            return None;
        }
        Some(
            self.turns
                .get(self.selected)?
                .choice
                .min(question.alternatives.len()),
        )
    }

    pub(super) fn choice_control(&self, key: Key) -> Option<Control> {
        let choice = self.selected_choice()?;
        let count = self.question()?.alternatives.len();
        match key {
            Key::Down | Key::Char('j') => Some(Control::SelectChoice((choice + 1).min(count))),
            Key::Up | Key::Char('k') => Some(Control::SelectChoice(choice.saturating_sub(1))),
            Key::Enter => Some(Control::Send),
            _ => None,
        }
    }
}
