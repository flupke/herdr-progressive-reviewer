use super::{ComposeScope, DraftKey, ExploreComponent, Reveal};
use comment_editor::CommentEditor;

impl ExploreComponent {
    pub(super) fn edit_current(&mut self, edit: impl FnOnce(&mut CommentEditor)) {
        let editor = match self.editor_target {
            super::EditorTarget::Answer => &mut self.editor,
            super::EditorTarget::Implementation => {
                let view = self.conclusion_mut().expect("implementation editor");
                if !view.can_edit() {
                    return;
                }
                &mut view.editor
            }
        };
        let before = editor.text();
        edit(editor);
        if editor.text() != before {
            self.implementation_changed();
        }
    }
    pub(super) fn composing_answer(&self, turn: usize, has_answers: bool) -> bool {
        !self.general_reply()
            && turn == self.selected
            && self.can_compose()
            && (!has_answers || self.editing || !self.editor.text().is_empty())
    }

    pub(super) fn edit_turn(&mut self, turn: usize) {
        if turn != self.selected || self.general_reply() {
            self.select(turn);
        }
        self.edit_answer();
    }

    pub(super) fn draft_key(&self) -> DraftKey {
        match self.compose_scope {
            ComposeScope::Question => DraftKey::Question(self.selected),
            ComposeScope::Conclusion => {
                DraftKey::Conclusion(self.general_context.clone().expect("conclusion context"))
            }
        }
    }

    pub(super) fn save_draft(&mut self) {
        if self.can_compose() {
            let editor = std::mem::replace(&mut self.editor, CommentEditor::new("", &self.keymap));
            self.drafts.insert(self.draft_key(), editor);
        }
    }

    pub(super) fn restore_draft(&mut self) {
        self.editor = self
            .drafts
            .remove(&self.draft_key())
            .unwrap_or_else(|| CommentEditor::new("", &self.keymap));
    }

    pub(super) fn edit_general(&mut self) {
        if self.general_context.is_none() {
            return;
        }
        if let Some(conclusion) = self.conclusion_mut() {
            conclusion.replying = true;
        }
        self.edit_answer();
        self.reveal
            .set(Some(Reveal::Editor(super::EditorTarget::Answer)));
    }
}
