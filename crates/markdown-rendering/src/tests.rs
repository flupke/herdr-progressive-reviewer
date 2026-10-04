use super::MarkdownRenderer;
use ratatui::style::Modifier;
use ui_theme::Theme;

#[path = "code_blocks.tests.rs"]
mod code_blocks;

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

fn rendered_text(source: &str) -> String {
    MarkdownRenderer::default()
        .render(source, 80, Theme::default().palette)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_callout_shows_its_title_in_place_of_its_marker() {
    for (source, expected) in [
        ("> [!TIP] Run it twice.", "Tip: Run it twice."),
        ("> [!WARNING]\n> Run it twice.", "Warning:"),
        ("> > [!ERROR] Nested.", "Error: Nested."),
    ] {
        let text = rendered_text(source);
        assert!(text.contains(expected), "{source}: {text}");
        assert!(!text.contains("[!"), "{source}: {text}");
    }
    assert!(rendered_text("> [!NOTE] Unknown.").contains("[!NOTE] Unknown."));
}

#[test]
fn a_marked_table_cell_shows_its_symbol_in_place_of_its_marker() {
    let text = rendered_text(
        "| Option | Speed |\n| --- | --- |\n| cache | [!good] 2 ms |\n| scan | [!bad] |\n| walk | [!warning] 9 ms |",
    );
    for expected in ["✓ 2 ms", "✗", "! 9 ms"] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
    assert!(!text.contains("[!"), "{text}");
}

#[test]
fn inline_code_inside_bold_italic_or_struck_text_shows_as_code_in_that_style() {
    let palette = Theme::default().palette;
    for (source, modifier) in [
        ("**The `cache` is new.**", Modifier::BOLD),
        ("__The `cache` is new.__", Modifier::BOLD),
        ("*The `cache` is new.*", Modifier::ITALIC),
        ("~~The `cache` is new.~~", Modifier::CROSSED_OUT),
    ] {
        let lines = MarkdownRenderer::default().render(source, 80, palette);
        let spans: Vec<_> = lines.iter().flat_map(|line| &line.spans).collect();
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text.trim(), "The cache is new.", "{source}");
        let code = spans
            .iter()
            .find(|span| span.content == "cache")
            .unwrap_or_else(|| panic!("{source}: {spans:?}"));
        assert!(code.style.add_modifier.contains(modifier), "{source}");
        assert_eq!(code.style.fg, Some(palette.warning), "{source}");
        let prose = spans
            .iter()
            .find(|span| span.content.contains("new"))
            .unwrap_or_else(|| panic!("{source}: {spans:?}"));
        assert!(prose.style.add_modifier.contains(modifier), "{source}");
    }
}
