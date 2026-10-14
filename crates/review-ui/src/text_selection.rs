//! The text the reviewer drags over, copied when the button is released.

use std::cell::RefCell;

use ratatui::layout::Position;
use ui_text_selection::{TextLayer, TextPress, TextSelection};

use crate::{Action, TerminalAction, UserInput};

/// The text regions of the last frame, and the text selection made over them.
#[derive(Default)]
pub(crate) struct ScreenText {
    layer: RefCell<TextLayer>,
    selection: Option<Selecting>,
    /// The terminal's size, which each redraw announces again.
    size: Option<(u16, u16)>,
}

enum Selecting {
    /// The button went down with Alt held: the drag is the text selection's alone until the
    /// release, even where the press found no text to select.
    AltDragging(Option<TextSelection>),
    /// The button went down on text where a plain drag means nothing else: the drag is the
    /// text selection's, and the press and the release go to the application too.
    PlainDragging(TextSelection),
    /// The text selection was copied, and stays shown until the next input.
    Copied(TextSelection),
}

/// What a text selection made of an input.
pub(crate) enum TextInput {
    /// The input was the text selection's alone, and ran these actions.
    Taken(Vec<Action>),
    /// The application takes the input too, after these actions.
    Passed(Vec<Action>),
}

impl ScreenText {
    /// Keep the text regions a frame laid out, for the next text selection.
    pub(crate) fn set_layer(&self, layer: TextLayer) {
        self.layer.replace(layer);
    }

    /// The text selection to paint over the frame.
    pub(crate) fn shown(&self) -> Option<&TextSelection> {
        match self.selection.as_ref()? {
            Selecting::AltDragging(selection) => selection.as_ref(),
            Selecting::PlainDragging(selection) | Selecting::Copied(selection) => Some(selection),
        }
    }

    /// Take the input of a text selection's drag, with the actions it runs. Other input
    /// drops a copied text selection, unless it is a redraw.
    pub(crate) fn input(&mut self, message: &UserInput) -> TextInput {
        if let UserInput::Resize { width, height } = message {
            self.resize((*width, *height));
            return TextInput::Passed(Vec::new());
        }
        match (message, self.selection.take()) {
            (UserInput::MouseAltClick { column, row }, _) => {
                let selection = self.select(*column, *row, TextPress::Alt);
                self.selection = Some(Selecting::AltDragging(selection));
                TextInput::Taken(Vec::new())
            }
            (UserInput::MouseClick { column, row }, _) => {
                self.selection = self
                    .select(*column, *row, TextPress::Plain)
                    .map(Selecting::PlainDragging);
                TextInput::Passed(Vec::new())
            }
            (UserInput::MouseDrag { column, row }, Some(mut dragging)) if dragging.is_drag() => {
                if let Selecting::AltDragging(Some(selection))
                | Selecting::PlainDragging(selection) = &mut dragging
                {
                    selection.extend(Position::new(*column, *row));
                }
                self.selection = Some(dragging);
                TextInput::Taken(Vec::new())
            }
            (UserInput::MouseRelease, Some(Selecting::AltDragging(selection))) => {
                TextInput::Taken(self.release(selection))
            }
            (UserInput::MouseRelease, Some(Selecting::PlainDragging(selection))) => {
                TextInput::Passed(self.release(Some(selection)))
            }
            (_, Some(dragging)) if dragging.is_drag() => {
                self.selection = Some(dragging);
                TextInput::Passed(Vec::new())
            }
            _ => TextInput::Passed(Vec::new()),
        }
    }

    /// Keep a copied text selection through a redraw, which announces the same size again.
    fn resize(&mut self, size: (u16, u16)) {
        let redraw = self.size.replace(size) == Some(size);
        if !redraw && matches!(self.selection, Some(Selecting::Copied(_))) {
            self.selection = None;
        }
    }

    /// End a drag, copying the text it selected, if any.
    fn release(&mut self, selection: Option<TextSelection>) -> Vec<Action> {
        let Some((selection, text)) =
            selection.and_then(|selection| selection.text().map(|text| (selection, text)))
        else {
            return Vec::new();
        };
        self.selection = Some(Selecting::Copied(selection));
        vec![Action::Terminal(TerminalAction::CopyToClipboard(text))]
    }

    fn select(&self, column: u16, row: u16, press: TextPress) -> Option<TextSelection> {
        self.layer
            .borrow()
            .select(Position::new(column, row), press)
    }
}

impl Selecting {
    /// Whether the button is still down.
    fn is_drag(&self) -> bool {
        !matches!(self, Self::Copied(_))
    }
}
