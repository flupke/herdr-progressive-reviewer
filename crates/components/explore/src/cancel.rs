//! Cancelling the reviewer's latest answer: the question it answered comes
//! back with the answer ready to edit and send again.

use review_explore::{Command, ExplorePage, ReviewerAnswer};
use ui_actions::Action;
use ui_events::ExploreAnswerCancelled;

use super::{ComposeScope, EditorTarget, ExploreComponent, Progress};
use comment_editor::CommentEditor;

impl ExploreComponent {
    /// Whether `answer` is the latest answer and can still be cancelled.
    pub(super) fn can_cancel(&self, answer: &ReviewerAnswer) -> bool {
        self.durable.enabled
            && !self.durable.historical
            && self.durable.error.is_none()
            && self.cancelling.is_none()
            && self.durable.posting.is_none()
            && !self.implementation_requested
            && !self.progress.awaiting_capture()
            && self
                .exploration
                .as_ref()
                .and_then(|exploration| exploration.answers.last())
                .is_some_and(|latest| latest.id == answer.id)
    }

    pub(super) fn cancel_answer(&mut self, index: usize) -> Vec<Action> {
        let Some(answer) = self
            .exploration
            .as_ref()
            .and_then(|exploration| exploration.answers.get(index))
            .filter(|answer| self.can_cancel(answer))
            .cloned()
        else {
            return Vec::new();
        };
        self.cancelling = Some(answer.id.clone());
        self.status = "Cancelling your answer…".into();
        vec![Action::Explore(Command::CancelAnswer(answer.id))]
    }

    pub(super) fn answer_cancelled(&mut self, event: &ExploreAnswerCancelled) {
        if self.cancelling.as_ref() != Some(&event.answer) {
            return;
        }
        self.cancelling = None;
        let pass = match &event.result {
            Ok(pass) => pass,
            Err(error) => {
                self.status = format!("Answer was not cancelled: {error}");
                return;
            }
        };
        let Some(answer) = self
            .exploration
            .as_ref()
            .and_then(|exploration| {
                exploration
                    .answers
                    .iter()
                    .find(|answer| answer.id == event.answer)
            })
            .cloned()
        else {
            return;
        };
        // Keep the current page's text before its page may disappear.
        self.save_draft();
        self.adopt(pass);
        self.durable.posting = None;
        self.forget_removed_pages();
        self.progress = Progress::Ready;
        self.status = "Answer cancelled. Change it and send it again.".into();
        self.expanded_marks.remove(&answer.id);
        self.reopen_answer(&answer);
    }

    /// Show the page `answer` answered with its choice and text ready to send.
    fn reopen_answer(&mut self, answer: &ReviewerAnswer) {
        let Some(page) = self
            .exploration
            .as_ref()
            .and_then(|exploration| exploration.answer_page(answer))
            .filter(|page| *page != ExplorePage::Opening)
        else {
            self.restore_draft();
            return;
        };
        match &page {
            ExplorePage::Question(index) => {
                self.open_question(*index);
                if let Some(choice) = answer.option.as_ref().and_then(|option| {
                    answer
                        .question
                        .as_ref()?
                        .choices()
                        .position(|choice| choice.id == option.id)
                }) {
                    self.turns[*index].choice = choice;
                }
            }
            ExplorePage::Conclusion(request) => {
                self.open_conclusion(request.clone());
                if let Some(view) = self.conclusion_mut() {
                    view.replying = true;
                }
                self.editor_target = EditorTarget::Answer;
            }
            ExplorePage::Opening => {}
        }
        self.drafts.remove(&page);
        self.editor = CommentEditor::new(&answer.text, &self.keymap);
    }

    /// Drop the drafts and conclusions of pages the cancelled turn posted.
    fn forget_removed_pages(&mut self) {
        let Some(exploration) = &self.exploration else {
            return;
        };
        let questions = exploration.questions.len();
        let concluded = |request: &String| {
            exploration
                .conversation
                .iter()
                .any(|turn| &turn.update.request == request && turn.update.conclusion.is_some())
        };
        self.conclusions.retain(|request, _| concluded(request));
        self.drafts.retain(|page, _| match page {
            ExplorePage::Question(index) => *index < questions,
            ExplorePage::Conclusion(request) => concluded(request),
            ExplorePage::Opening => false,
        });
        if self.compose_scope == ComposeScope::Conclusion
            && self
                .general_context
                .as_ref()
                .is_none_or(|request| !self.conclusions.contains_key(request))
        {
            self.compose_scope = ComposeScope::Question;
            self.general_context = None;
        }
        self.selected = self.selected.min(questions.saturating_sub(1));
    }
}
