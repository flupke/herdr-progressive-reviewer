use super::*;

fn screen(text: &str) -> Screen {
    let marker = FrameMarker {
        frame: 1,
        acknowledged: 0,
        columns: 20,
        rows: 3,
    };
    Screen::new(&marker, text.into(), None)
}

#[test]
fn text_is_located_in_terminal_columns() {
    let screen = screen("Files\n日本語 🦀 notes.md\n");
    assert_eq!(screen.locate("notes").unwrap(), Cell { x: 10, y: 1 });
    assert_eq!(screen.locate("Files").unwrap(), Cell { x: 0, y: 0 });
}

#[test]
fn missing_or_empty_text_cannot_be_located() {
    let screen = screen("Files");
    assert_eq!(
        screen.locate("Threads").unwrap_err().to_string(),
        "the screen does not show \"Threads\""
    );
    assert!(screen.locate("").is_err());
}

#[test]
fn cells_outside_the_screen_are_refused() {
    let screen = screen("Files");
    assert!(screen.contains(Cell { x: 19, y: 2 }).is_ok());
    assert_eq!(
        screen
            .contains(Cell { x: 20, y: 0 })
            .unwrap_err()
            .to_string(),
        "cell (20, 0) is outside the 20x3 screen"
    );
}

#[test]
fn a_left_click_is_a_one_based_sgr_press_and_release() {
    assert_eq!(Cell { x: 4, y: 2 }.left_click(), "\x1b[<0;5;3M\x1b[<0;5;3m");
}
