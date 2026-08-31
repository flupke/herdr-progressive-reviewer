//! Terminal input accepted by the review application.

use ui_shortcuts::Key;

/// One normalized terminal input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UserInput {
    Resize {
        width: u16,
        height: u16,
    },
    MouseScroll {
        column: u16,
        row: u16,
        delta: isize,
    },
    MouseClick {
        column: u16,
        row: u16,
        insert_path: bool,
    },
    MouseControlClick {
        column: u16,
        row: u16,
    },
    MouseDoubleClick {
        column: u16,
        row: u16,
    },
    MouseRightClick {
        column: u16,
        row: u16,
    },
    MouseDrag {
        column: u16,
        row: u16,
    },
    MouseRelease,
    Key(Key),
}
