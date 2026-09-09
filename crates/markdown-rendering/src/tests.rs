use super::MarkdownRenderer;
use ui_theme::Theme;

#[test]
fn inline_code_preserves_adjacent_punctuation_and_source_spacing() {
    let palette = Theme::default().palette;
    let renderer = MarkdownRenderer::default();
    for (source, expected) in [
        ("`stuff`.", "stuff."),
        ("(`stuff`), then `other`;", "(stuff), then other;"),
        ("prefix`code`suffix", "prefixcodesuffix"),
        ("Use `code` here.", "Use code here."),
        ("**Bold**`code`.", "Boldcode."),
    ] {
        let lines = renderer.render(source, 80, palette);
        let text = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text.trim(), expected, "{source}");
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.style.fg == Some(palette.warning))
        );
    }
}

#[test]
fn inline_code_adds_no_width_before_wrapping() {
    let lines = MarkdownRenderer::default().render("`stuff`.", 6, Theme::default().palette);
    let text = lines
        .iter()
        .map(ToString::to_string)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(text, ["stuff."]);
}

#[test]
fn inline_code_spacing_is_consistent_in_markdown_blocks() {
    let renderer = MarkdownRenderer::default();
    let palette = Theme::default().palette;
    for source in [
        "# (`code`).",
        "- (`code`).",
        "> (`code`).",
        "| Value |\n| --- |\n| (`code`). |",
    ] {
        let text = renderer
            .render(source, 80, palette)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("(code)."), "{source}: {text}");
    }
}
