use super::*;

#[test]
fn a_dim_placeholder_is_an_empty_box_and_typed_text_is_not() {
    let placeholder = "❯ an earlier prompt\n────\n❯\u{a0}\u{1b}[0m\u{1b}[2mTry \"how do I log an error?\"\u{1b}[0m\n────\n  ? for shortcuts\n";
    let boxed = "╭────╮\n│ ❯ \u{1b}[2mSuggested prompt\u{1b}[22m │\n╰────╯\n";
    let draft = "────\n❯\u{a0}half a \u{1b}[1mthought\u{1b}[0m\n────\n";

    assert_eq!(input_box_text(placeholder).as_deref(), Some(""));
    assert_eq!(input_box_text(boxed).as_deref(), Some(""));
    assert_eq!(input_box_text(draft).as_deref(), Some("half a thought"));
    assert_eq!(input_box_text("Resuming the conversation…"), None);
}
