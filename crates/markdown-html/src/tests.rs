use super::*;

fn render(markdown: &str) -> String {
    HtmlRenderer::under_heading(2)
        .render(markdown)
        .into_string()
}

#[test]
fn paragraphs_lists_and_inline_code_render_as_html() {
    let html = render("The cache holds `entries`.\n\n- first\n- second\n\n1. one\n2. two");
    assert!(
        html.contains("<p>The cache holds <code>entries</code>.</p>"),
        "{html}"
    );
    assert!(
        html.contains("<ul>\n<li>first</li>\n<li>second</li>\n</ul>"),
        "{html}"
    );
    assert!(
        html.contains("<ol>\n<li>one</li>\n<li>two</li>\n</ol>"),
        "{html}"
    );
}

#[test]
fn a_fenced_block_names_its_language() {
    let html = render("```mermaid\ngraph LR\n  a --> b\n```\n\n```\nplain\n```");
    assert!(
        html.contains(
            "<pre><code class=\"language-mermaid\">graph LR\n  a --&gt; b\n</code></pre>"
        ),
        "{html}"
    );
    assert!(html.contains("<pre><code>plain\n</code></pre>"), "{html}");
}

#[test]
fn headings_start_one_level_below_the_heading_they_sit_under() {
    let html = render("# Top\n\n## Next\n\n###### Deepest");
    assert!(html.contains("<h3>Top</h3>"), "{html}");
    assert!(html.contains("<h4>Next</h4>"), "{html}");
    assert!(html.contains("<h6>Deepest</h6>"), "{html}");
}

#[test]
fn a_table_keeps_its_alignment_without_inline_styles() {
    let html = render("| Step | Cost |\n| :--- | ---: |\n| read | 2 ms |");
    assert!(
        html.contains("<th class=\"align-left\">Step</th><th class=\"align-right\">Cost</th>"),
        "{html}"
    );
    assert!(
        html.contains("<td class=\"align-left\">read</td><td class=\"align-right\">2 ms</td>"),
        "{html}"
    );
    assert!(!html.contains("style"), "{html}");
}

#[test]
fn a_marked_cell_shows_its_status_in_place_of_the_marker() {
    let html = render(
        "| Option | Speed | Memory |\n| --- | --- | --- |\n| cache | [!good] 2 ms | [!bad] |\n| scan | [!warning] 40 ms | plain |",
    );
    assert!(
        html.contains(
            "<td class=\"status-good\"><span class=\"status\" role=\"img\" aria-label=\"Good\">✓</span> 2 ms</td>"
        ),
        "{html}"
    );
    assert!(
        html.contains(
            "<td class=\"status-bad\"><span class=\"status\" role=\"img\" aria-label=\"Bad\">✗</span></td>"
        ),
        "{html}"
    );
    assert!(
        html.contains("aria-label=\"Warning\">!</span> 40 ms</td>"),
        "{html}"
    );
    assert!(html.contains("<td>plain</td>"), "{html}");
    assert!(!html.contains("[!"), "{html}");
}

#[test]
fn a_status_mark_stays_apart_from_the_markup_after_it() {
    let html = render("| Speed |\n| --- |\n| [!good] **2 ms** |");
    assert!(
        html.contains("aria-label=\"Good\">✓</span> <strong>2 ms</strong></td>"),
        "{html}"
    );
}

#[test]
fn a_quote_that_opens_with_a_callout_marker_is_a_callout() {
    for (markdown, kind, label) in [
        ("> [!TIP] Run it twice.", "tip", "Tip"),
        ("> [!WARNING]\n> Run it twice.", "warning", "Warning"),
        ("> [!error]\n>\n> Run it twice.", "error", "Error"),
        (
            "> [!CONCLUSION]\n> Run it twice.",
            "conclusion",
            "Conclusion",
        ),
    ] {
        let html = render(markdown);
        assert!(
            html.starts_with(&format!(
                "<div class=\"callout callout-{kind}\" role=\"note\" aria-label=\"{label}\">\n<p class=\"callout-title\">{label}</p>\n<p>Run it twice.</p>\n</div>"
            )),
            "{markdown}: {html}"
        );
        assert!(!html.contains("blockquote"), "{html}");
    }
}

#[test]
fn a_callout_keeps_the_rest_of_its_content() {
    let html = render("> [!TIP] First.\n> Second line.\n>\n> - a list\n\nAfter.");
    assert!(html.contains("<p>First.\nSecond line.</p>"), "{html}");
    assert!(html.contains("<li>a list</li>"), "{html}");
    assert!(html.ends_with("</div>\n<p>After.</p>\n"), "{html}");
}

#[test]
fn a_plain_quote_and_an_unknown_marker_stay_quotes() {
    let html = render("> Quoted.\n\n> [!NOTE] Unknown.\n\n> > [!TIP] Nested.");
    assert!(
        html.contains("<blockquote>\n<p>Quoted.</p>\n</blockquote>"),
        "{html}"
    );
    assert!(html.contains("<p>[!NOTE] Unknown.</p>"), "{html}");
    assert!(
        html.contains("<blockquote>\n<div class=\"callout callout-tip\""),
        "{html}"
    );
}

#[test]
fn raw_html_shows_as_text() {
    let html = render(
        "<script>alert(1)</script>\n\nInline <img src=x onerror=alert(1)> and <b>bold</b>.\n\n<div onclick=\"x()\">\nblock\n</div>",
    );
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("<img"), "{html}");
    assert!(!html.contains("<b>"), "{html}");
    assert!(!html.contains("<div"), "{html}");
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{html}"
    );
    assert!(
        html.contains("Inline &lt;img src=x onerror=alert(1)&gt; and &lt;b&gt;bold&lt;/b&gt;."),
        "{html}"
    );
}

#[test]
fn only_web_links_stay_links_and_images_show_their_description() {
    let html = render(
        "[docs](https://example.com/a?b=1&c=2), [mail](mailto:a@example.com), <b@example.com>, [run](javascript:alert(1)), <javascript:alert(2)>, [file](src/lib.rs) and ![a diagram](https://example.com/x.png)",
    );
    assert!(
        html.contains("<a href=\"https://example.com/a?b=1&amp;c=2\">docs</a>"),
        "{html}"
    );
    assert!(
        html.contains("<a href=\"mailto:a@example.com\">mail</a>"),
        "{html}"
    );
    assert!(
        html.contains("<a href=\"mailto:b@example.com\">b@example.com</a>"),
        "{html}"
    );
    assert!(!html.contains("javascript:alert(1)\""), "{html}");
    assert!(!html.contains("href=\"javascript"), "{html}");
    assert!(!html.contains("href=\"src"), "{html}");
    assert!(html.contains(", run, "), "{html}");
    assert!(html.contains(" file and a diagram</p>"), "{html}");
    assert!(!html.contains("<img"), "{html}");
}

#[test]
fn a_line_renders_as_inline_html_with_no_block_around_it() {
    let html = HtmlRenderer::under_heading(2)
        .render_inline(
            "`ReplyQueue` holds **each** reply\nuntil [the policy](https://example.com) sends it",
        )
        .into_string();
    assert_eq!(
        html,
        "<code>ReplyQueue</code> holds <strong>each</strong> reply until \
         <a href=\"https://example.com\">the policy</a> sends it"
    );
}

#[test]
fn a_line_that_looks_like_a_block_keeps_only_its_text() {
    let render = |markdown| {
        HtmlRenderer::under_heading(2)
            .render_inline(markdown)
            .into_string()
    };
    assert_eq!(render("# A heading"), "A heading");
    assert_eq!(render("- an item"), "an item");
    assert_eq!(render("> [!WARNING] careful"), "careful");
    assert_eq!(render("<b>raw</b>"), "&lt;b&gt;raw&lt;/b&gt;");
}
