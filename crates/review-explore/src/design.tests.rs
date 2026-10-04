use super::{Design, DesignPart};
use serde_json::json;

fn shown(design: &Design) -> Vec<(&'static str, String, String)> {
    design
        .parts()
        .into_iter()
        .map(|part| (part.title, part.thesis.into_owned(), part.body.into_owned()))
        .collect()
}

#[test]
fn the_agent_states_a_thesis_for_the_change_and_for_each_part() {
    let design: Design = serde_json::from_value(json!({
        "thesis": "A cache in front of the parser skips files that did not change.",
        "overview": {"thesis": "The cache sits between the reader and the parser.", "body": "It lives in cache.rs."},
        "data_flow": {"thesis": "Source text flows through the cache.", "body": "A hit returns the tree."},
        "algorithm": {"thesis": "One hash lookup per file.", "body": "| Step | Cost |\n|---|---|\n| hash | O(n) |"},
        "alternatives": {"thesis": "No cache was rejected as slow.", "body": "Measured at 2 s."},
    }))
    .unwrap();

    assert_eq!(
        design.thesis(),
        "A cache in front of the parser skips files that did not change."
    );
    assert_eq!(
        shown(&design),
        [
            (
                "What it adds and where",
                "The cache sits between the reader and the parser.".into(),
                "It lives in cache.rs.".into()
            ),
            (
                "Types and data flow",
                "Source text flows through the cache.".into(),
                "A hit returns the tree.".into()
            ),
            (
                "Algorithm and cost",
                "One hash lookup per file.".into(),
                "| Step | Cost |\n|---|---|\n| hash | O(n) |".into()
            ),
            (
                "Rejected alternatives",
                "No cache was rejected as slow.".into(),
                "Measured at 2 s.".into()
            ),
        ]
    );
    let saved = serde_json::to_value(&design).unwrap();
    assert_eq!(serde_json::from_value::<Design>(saved).unwrap(), design);
}

#[test]
fn a_design_saved_before_theses_shows_each_parts_first_paragraph_as_its_thesis() {
    let design: Design = serde_json::from_value(json!({
        "overview": "The cache sits between the reader\nand the parser.\n\nIt lives in cache.rs.",
        "data_flow": "```mermaid\nflowchart TD\n  A --> B\n```\n\nSource text flows through the cache.\n\nA hit returns the tree.",
        "algorithm": "One hash lookup per file.",
        "alternatives": "- No cache\n- A database",
    }))
    .unwrap();

    assert_eq!(
        shown(&design),
        [
            // Its first paragraph stands in for the change's thesis, its next one for its own.
            (
                "What it adds and where",
                "It lives in cache.rs.".into(),
                String::new()
            ),
            (
                "Types and data flow",
                "Source text flows through the cache.".into(),
                "```mermaid\nflowchart TD\n  A --> B\n```\n\nA hit returns the tree.".into()
            ),
            (
                "Algorithm and cost",
                "One hash lookup per file.".into(),
                String::new()
            ),
            // No paragraph: the first line of text stands in, and the part keeps all its text.
            (
                "Rejected alternatives",
                "No cache".into(),
                "- No cache\n- A database".into()
            ),
        ]
    );
    // The overview says what the change adds: its first paragraph, on one line.
    assert_eq!(
        design.thesis(),
        "The cache sits between the reader and the parser."
    );
    // Saved again, it loads the same, still without theses.
    let saved = serde_json::to_value(&design).unwrap();
    assert_eq!(serde_json::from_value::<Design>(saved).unwrap(), design);
}

#[test]
fn a_saved_part_that_is_only_code_shows_its_first_line_as_its_thesis() {
    let part: DesignPart =
        serde_json::from_value(json!("```\nlet cache = Cache::new();\n```")).unwrap();

    let text = part.text();
    assert_eq!(text.thesis, "let cache = Cache::new();");
    assert_eq!(text.body, "```\nlet cache = Cache::new();\n```");
}

#[test]
fn a_part_the_agent_sends_with_an_unknown_field_is_refused() {
    let refused = serde_json::from_value::<DesignPart>(
        json!({"thesis": "One lookup.", "body": "Text.", "summary": "Extra."}),
    );

    assert!(refused.unwrap_err().to_string().contains("summary"));
}

#[test]
fn a_saved_overview_of_one_paragraph_gives_it_to_the_change_and_to_the_part() {
    let design: Design = serde_json::from_value(json!({
        "overview": "The cache sits before the parser.",
        "data_flow": "Text.", "algorithm": "Text.", "alternatives": "Text.",
    }))
    .unwrap();

    assert_eq!(design.thesis(), "The cache sits before the parser.");
    let [overview, ..] = design.parts();
    assert_eq!(overview.thesis, "The cache sits before the parser.");
    assert_eq!(overview.body, "");
}

#[test]
fn a_paragraph_in_a_quote_or_a_list_stays_in_its_block() {
    let design: Design = serde_json::from_value(json!({
        "overview": "> The cache sits\n> before the parser.\n\nIt lives in \\*cache\\*.rs,\nbeside the parser.",
        "data_flow": "- Text flows in.\n\n  A hit returns the tree.\n\nThe cache keeps the tree.",
        "algorithm": "Text.", "alternatives": "Text.",
    }))
    .unwrap();

    // The first paragraph of its own, on one line, as written.
    assert_eq!(
        design.thesis(),
        "It lives in \\*cache\\*.rs, beside the parser."
    );
    let [overview, data_flow, ..] = design.parts();
    // Nothing follows it, so the overview shares the change's thesis, and keeps its quote.
    assert_eq!(overview.thesis, design.thesis());
    assert_eq!(overview.body, "> The cache sits\n> before the parser.");
    assert_eq!(data_flow.thesis, "The cache keeps the tree.");
    assert_eq!(
        data_flow.body,
        "- Text flows in.\n\n  A hit returns the tree."
    );
}

#[test]
fn a_saved_part_without_a_paragraph_shows_its_first_line_as_written() {
    for (body, thesis) in [
        ("- Uses `foo` to do x\n- Other", "Uses `foo` to do x"),
        (
            "> **Polling**: rejected as slow.",
            "**Polling**: rejected as slow.",
        ),
        ("- \\*star\\* item", "\\*star\\* item"),
    ] {
        let part: DesignPart = serde_json::from_value(json!(body)).unwrap();
        assert_eq!(part.text().thesis, thesis);
    }
    let part: DesignPart = serde_json::from_value(json!("First line,\\\nthen the rest.")).unwrap();
    assert_eq!(part.text().thesis, "First line, then the rest.");
}

/// A design whose parts hold `words` words of prose each, after one-word theses.
fn design_of(words: usize, extra: &str) -> Design {
    let body = format!("{}\n\n{extra}", "word ".repeat(words));
    let part = || DesignPart::new("Thesis.", body.clone());
    Design::new("Thesis.", part(), part(), part(), part())
}

#[test]
fn the_reading_time_counts_the_words_of_the_theses_and_the_parts() {
    // 5 theses and 4 × 280 words: 1,125 words, about 4.9 minutes at 230 words a minute.
    assert_eq!(design_of(280, "").reading_minutes(), 5);
    // 5 + 4 × 200 = 805 words: 3.5 minutes, rounded up.
    assert_eq!(design_of(200, "").reading_minutes(), 4);
}

#[test]
fn the_words_of_a_table_and_of_inline_code_are_read() {
    // 1 + 146 + 2 words in each part's table, as many as in a paragraph of 149 words.
    let table = format!(
        "| `ReplyQueue` | {}|\n| --- | --- |\n| a | b |",
        "word ".repeat(146)
    );
    assert_eq!(design_of(0, &table).reading_minutes(), 3);
    assert_eq!(design_of(149, "").reading_minutes(), 3);
}

#[test]
fn a_diagram_source_does_not_count_as_reading() {
    let diagram = format!(
        "```mermaid\nsequenceDiagram\n{}```",
        "  a->>b: says one more thing\n".repeat(200)
    );
    assert_eq!(
        design_of(200, &diagram).reading_minutes(),
        design_of(200, "").reading_minutes()
    );
}

#[test]
fn a_short_design_takes_about_a_minute() {
    assert_eq!(design_of(1, "").reading_minutes(), 1);
}
