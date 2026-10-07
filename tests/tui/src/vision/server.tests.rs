use vision_signal::FrameMarker;

use super::*;

fn screen(frame: u64, columns: u16, text: &str) -> Screen {
    sized(frame, columns, 2, text)
}

fn sized(frame: u64, columns: u16, rows: u16, text: &str) -> Screen {
    let marker = FrameMarker {
        frame,
        acknowledged: 0,
        columns,
        rows,
    };
    Screen::new(&marker, text.into(), None)
}

/// The result's summary, and its other texts.
fn read(result: &CallToolResult) -> (Value, Vec<String>) {
    let mut texts = result
        .content
        .iter()
        .filter_map(|content| content.as_text())
        .map(|text| text.text.clone());
    let summary = serde_json::from_str(&texts.next().unwrap()).unwrap();
    (summary, texts.collect())
}

#[test]
fn a_reaction_returns_the_rows_that_changed_since_the_last_screen() {
    let mut shown = Some(screen(1, 20, "Files\na.rs"));
    let result = Outcome::reaction("reacted", screen(2, 20, "Files\nb.rs")).into_result(&mut shown);
    let (summary, texts) = read(&result);
    assert_eq!(summary["changed_rows"], 1);
    assert_eq!(texts, ["1: b.rs"]);
    assert_eq!(shown.unwrap().text, "Files\nb.rs");
}

#[test]
fn a_first_reaction_returns_the_whole_screen() {
    let mut shown = None;
    let result = Outcome::reaction("reacted", screen(2, 20, "Files\nb.rs")).into_result(&mut shown);
    let (summary, texts) = read(&result);
    assert!(summary.get("changed_rows").is_none(), "{summary}");
    assert_eq!(texts, ["0: Files\n1: b.rs"]);
    assert_eq!(shown.unwrap().frame, 2);
}

#[test]
fn a_reaction_after_a_resize_returns_the_whole_screen() {
    for (columns, rows) in [(30, 2), (20, 3)] {
        let mut shown = Some(sized(1, columns, rows, "Files\na.rs"));
        let result =
            Outcome::reaction("reacted", screen(2, 20, "Files\nb.rs")).into_result(&mut shown);
        let (summary, texts) = read(&result);
        assert!(summary.get("changed_rows").is_none(), "{columns}x{rows}: {summary}");
        assert_eq!(texts, ["0: Files\n1: b.rs"], "{columns}x{rows}");
    }
}

#[test]
fn the_screen_tool_returns_the_whole_screen_after_another() {
    let mut shown = Some(screen(1, 20, "Files\na.rs"));
    let result = Outcome::screen("screen", screen(2, 20, "Files\nb.rs")).into_result(&mut shown);
    let (summary, texts) = read(&result);
    assert!(summary.get("changed_rows").is_none(), "{summary}");
    assert_eq!(texts, ["0: Files\n1: b.rs"]);
}

#[test]
fn a_hidden_screen_returns_no_text_and_leaves_the_last_screen() {
    let mut shown = Some(screen(1, 20, "Files\na.rs"));
    let result = Outcome {
        show: Show::Hidden,
        ..Outcome::screen("screenshot", screen(2, 20, "Files\nb.rs"))
    }
    .into_result(&mut shown);
    let (summary, texts) = read(&result);
    assert_eq!(summary["frame"], 2);
    assert!(texts.is_empty(), "{texts:?}");
    assert_eq!(shown.unwrap().frame, 1);
}
