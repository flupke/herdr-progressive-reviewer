//! Shared pointer-input behavior for modal components.

use component_core::{InputMatcher, InputResolution};
use ratatui::layout::{Position, Rect};
use ui_events::{PointerInput, PointerInputKind};

/// A component that can report the active modal content area.
pub trait ModalComponent {
    /// Return the active modal content area, including its border.
    fn modal_area(&self) -> Option<Rect>;
}

/// Pointer input classified relative to the active modal content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModalPointerInput {
    /// Input that the modal must receive without dismissal.
    Deliver(PointerInput),
    /// A primary click outside the modal content.
    Dismiss,
}

/// Classify pointer input with the active modal content area.
pub struct ModalPointerInputMatcher;

impl<C> InputMatcher<C, PointerInput> for ModalPointerInputMatcher
where
    C: ModalComponent,
{
    type Output = ModalPointerInput;

    fn resolve(&mut self, component: &C, input: &PointerInput) -> InputResolution<Self::Output> {
        let Some(modal_area) = component.modal_area() else {
            return InputResolution::NoMatch;
        };
        let outside_click = matches!(input.kind, PointerInputKind::Click { .. })
            && input.position.is_some_and(|position| {
                !modal_area.contains(Position::new(
                    position.terminal_column,
                    position.terminal_row,
                ))
            });
        InputResolution::Matched(if outside_click {
            ModalPointerInput::Dismiss
        } else {
            ModalPointerInput::Deliver(*input)
        })
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
