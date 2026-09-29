//! Right-aligned thread controls share rendering and pointer geometry.

use ratatui::text::{Line, Span};
use review_threads::{Resolution, ReviewThread, ThreadId};

use super::{CommentRow, thread::ThreadLayout};
use crate::{
    comments::{CommentTarget, ConversationButton, EditorAction},
    conversation::ConversationAction,
};
use ui_controls::{ActionButton, ButtonTone};

/// The draft whose Cancel and Post buttons a control row carries.
#[derive(Clone, Copy)]
pub(super) struct DraftControls<'a> {
    pub(super) draft: &'a ThreadId,
    pub(super) focused: bool,
}

struct ThreadControl {
    label: String,
    action: ConversationAction,
    style: ButtonTone,
}

impl ThreadControl {
    fn new(
        full: String,
        short: &str,
        width: usize,
        action: ConversationAction,
        style: ButtonTone,
    ) -> Self {
        let label = if Line::raw(full.as_str()).width() <= width {
            full
        } else {
            short.chars().take(width).collect()
        };
        Self {
            label,
            action,
            style,
        }
    }

    fn editor(action: EditorAction, draft: &ThreadId, width: usize) -> Self {
        let (label, style) = match action {
            EditorAction::Submit => (" Post ", ButtonTone::Primary),
            EditorAction::Cancel => (" Cancel ", ButtonTone::Secondary),
        };
        Self::new(
            label.into(),
            label,
            width,
            ConversationAction::Editor(action, draft.clone()),
            style,
        )
    }

    fn resolution(thread: &ReviewThread, width: usize) -> Self {
        let verb = if thread.resolution == Resolution::Resolved {
            "Unresolve"
        } else {
            "Resolve"
        };
        Self::new(
            format!(" {verb} thread "),
            &format!(" {verb} "),
            width,
            ConversationAction::Resolve(thread.id.clone()),
            ButtonTone::Secondary,
        )
    }

    fn width(&self) -> usize {
        Line::raw(self.label.as_str()).width()
    }
}

impl ThreadLayout {
    pub(super) fn controls(
        self,
        rows: &mut Vec<CommentRow>,
        thread: Option<&ReviewThread>,
        draft: Option<DraftControls<'_>>,
    ) {
        let width = usize::from(self.frame.content_width());
        if width == 0 {
            return;
        }
        let mut controls = Vec::new();
        if let Some(thread) = thread {
            if draft.is_none() && thread.resolution == Resolution::Open {
                controls.push(ThreadControl::new(
                    " Retry agent ".into(),
                    " Retry ",
                    width,
                    ConversationAction::Retry(thread.id.clone()),
                    ButtonTone::Secondary,
                ));
            }
            controls.push(ThreadControl::resolution(thread, width));
        }
        if let Some(draft) = draft {
            controls.extend(
                [EditorAction::Cancel, EditorAction::Submit]
                    .map(|action| ThreadControl::editor(action, draft.draft, width)),
            );
        }
        if controls.is_empty() {
            return;
        }
        let total = controls.iter().map(ThreadControl::width).sum::<usize>() + controls.len() - 1;
        for group in controls.chunks(if total <= width { controls.len() } else { 1 }) {
            self.control_row(rows, group, width, draft.is_some_and(|draft| draft.focused));
        }
    }

    fn control_row(
        self,
        rows: &mut Vec<CommentRow>,
        controls: &[ThreadControl],
        width: usize,
        editing: bool,
    ) {
        let used = controls.iter().map(ThreadControl::width).sum::<usize>() + controls.len() - 1;
        let mut line = Line::raw(" ".repeat(width.saturating_sub(used)));
        let mut buttons = Vec::new();
        for control in controls {
            if !buttons.is_empty() {
                line.spans.push(Span::raw(" "));
            }
            let start = self.frame.content_column() + line.width();
            buttons.push(ConversationButton {
                action: control.action.clone(),
                columns: start..start + control.width(),
            });
            line.spans.push(
                ActionButton::new(control.label.trim(), control.style)
                    .span_with_text(control.label.clone(), self.palette),
            );
        }
        rows.extend(
            self.frame
                .wrapped_content(&line, self.source_row)
                .into_iter()
                .map(|rendered| CommentRow {
                    rendered,
                    target: Some(CommentTarget::ConversationButtons(buttons.clone())),
                    editor: editing,
                    reply: None,
                }),
        );
    }
}
