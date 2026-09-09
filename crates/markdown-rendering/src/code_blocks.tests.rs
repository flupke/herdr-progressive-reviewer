use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Widget};
use syntax_highlighting::SyntaxHighlighter;

use super::*;

#[test]
fn text_fences_render_complete_frames_and_preserve_layout() {
    let source = "Copied journal:\n  operations: (seed, local_id)\n\n  chunks: version -> counter";
    let palette = Theme::default().palette;
    let lines = MarkdownRenderer::default().render(&format!("```text\n{source}\n```"), 40, palette);
    let area = Rect::new(0, 0, 40, u16::try_from(lines.len()).unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(lines).render(area, &mut buffer);
    assert_eq!(buffer[(0, 0)].symbol(), "╭");
    assert_eq!(buffer[(39, 0)].symbol(), "╮");
    assert_eq!(buffer[(0, area.height - 1)].symbol(), "╰");
    assert_eq!(buffer[(39, area.height - 1)].symbol(), "╯");
    let mut body = Vec::new();
    for row in 1..area.height - 1 {
        assert_eq!(buffer[(0, row)].symbol(), "│");
        assert_eq!(buffer[(39, row)].symbol(), "│");
        body.push(
            (2..38)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned(),
        );
    }
    assert_eq!(body.join("\n"), source);
    assert_eq!(buffer[(0, 1)].fg, palette.dim);
    assert_eq!(buffer[(2, 1)].fg, palette.warning);
}

#[test]
fn narrow_code_frames_wrap_unicode_without_losing_text_or_syntax_styles() {
    let theme = Theme::default();
    let highlighter = SyntaxHighlighter::new(theme.syntax, theme.palette.text);
    let renderer = MarkdownRenderer::new(highlighter.clone());
    let source = "let café = \"界💬\";";
    let expected = highlighter
        .highlight_snippet("rust", source)
        .into_iter()
        .flatten()
        .flat_map(|token| {
            token
                .text
                .chars()
                .map(move |c| (c, token.color))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for width in [6, 12, 24, 80] {
        let lines = renderer.render(&format!("```rust\n{source}\n```"), width, theme.palette);
        assert!(lines.iter().all(|line| line.width() == usize::from(width)));
        let actual = lines[1..lines.len() - 1]
            .iter()
            .flat_map(|line| &line.spans[1..line.spans.len() - 1])
            .flat_map(|span| {
                span.content
                    .chars()
                    .map(|c| (c, span.style.fg.unwrap()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "width {width}");
        assert!(lines.first().unwrap().to_string().ends_with('╮'));
        assert!(lines.last().unwrap().to_string().ends_with('╯'));
    }
}

#[test]
fn quoted_code_frames_reserve_space_for_the_quote_prefix() {
    let lines = MarkdownRenderer::default().render(
        "> ```text\n>   operations: (seed, local_id)\n> ```",
        24,
        Theme::default().palette,
    );
    let text = lines.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert!(lines.iter().all(|line| line.width() <= 24), "{text:?}");
    assert!(text.first().unwrap().starts_with("│ ╭─ text"));
    assert!(text.first().unwrap().ends_with('╮'));
    assert!(text.last().unwrap().starts_with("│ ╰"));
    assert!(text.last().unwrap().ends_with('╯'));
}

#[test]
fn joined_letters_keep_the_right_border_in_its_terminal_column() {
    let lines = MarkdownRenderer::default().render("```لا\nلا\n```", 16, Theme::default().palette);
    let area = Rect::new(0, 0, 16, u16::try_from(lines.len()).unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(lines).render(area, &mut buffer);
    assert_eq!(buffer[(15, 0)].symbol(), "╮");
    assert_eq!(buffer[(15, 1)].symbol(), "│");
    assert_eq!(buffer[(15, 2)].symbol(), "╯");
}

#[test]
fn very_small_panes_and_empty_fences_do_not_overflow() {
    let renderer = MarkdownRenderer::default();
    for width in 0..12 {
        for source in ["```text\n\n```", "```a_long_language_name\n界 abc\n```"] {
            let lines = renderer.render(source, width, Theme::default().palette);
            assert!(
                lines
                    .iter()
                    .all(|line| line.width() <= usize::from(width.max(1))),
                "width {width}: {lines:?}"
            );
        }
    }
}
