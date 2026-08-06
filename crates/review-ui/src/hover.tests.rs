use super::{HoverView, ReviewApp};

#[test]
fn hover_hides_fences_and_uses_source_syntax_colors() {
    let app = ReviewApp::default();
    let markdown = "```rust\n\tpub struct Example;\n```";
    let lines = HoverView(&app, markdown).lines(80);
    let text = lines
        .iter()
        .flat_map(|line| &line.spans)
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let expected = app
        .highlighter
        .highlight_snippet("rust", "pub struct Example;\n")[0][0]
        .color;
    let actual = lines
        .iter()
        .flat_map(|line| &line.spans)
        .find(|span| span.content == "pub")
        .and_then(|span| span.style.fg);

    assert!(!text.contains("```"));
    assert!(text.contains("    pub struct Example;"));
    assert_eq!(actual, Some(expected));
}
