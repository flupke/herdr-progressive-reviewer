use std::sync::Arc;

use component_core::ComponentEventBus;
use ratatui::layout::Rect;
use review_explore::{
    CodeLocation, Comparison, Exploration, ReopenedLines, ReviewerAnswer, SourceSide, TurnMarks,
};
use review_repository::repository::RepoPath;
use review_source::{ReviewCheckpoint, SourceLineRange};
use review_types::MarkAuthor;
use ui_actions::Action;

use crate::ExploreComponent;
use crate::flow::{Content, ConversationLayout};

fn at(side: SourceSide, first_line: u32, last_line: u32) -> CodeLocation {
    CodeLocation {
        path: RepoPath::from_bytes(b"src/lib.rs"),
        side,
        lines: Some(SourceLineRange {
            first_line,
            last_line,
        }),
    }
}

fn answer() -> ReviewerAnswer {
    ReviewerAnswer {
        id: "answer".into(),
        checkpoint: ReviewCheckpoint::new("review", "checkpoint"),
        question: None,
        in_reply_to: "kickoff".into(),
        option: None,
        text: "Keep it.".into(),
        author: "reviewer".into(),
    }
}

/// An agent turn for `request`, following `answer` when it has one.
fn turn(request: &str, answer: Option<&str>) -> review_explore::ConversationTurn {
    review_explore::ConversationTurn {
        answer: answer.map(str::to_owned),
        update: serde_json::from_value(serde_json::json!({
            "instance": "round", "request": request,
            "checkpoint": {"review_unit": "review", "checkpoint": "checkpoint"},
            "interpretation": null, "topics": [], "next": null, "conclusion": null,
            "limitations": [], "findings": []
        }))
        .unwrap(),
    }
}

/// The text of each laid-out row: control labels and text lines.
fn rows(layout: &ConversationLayout) -> Vec<String> {
    layout
        .items
        .iter()
        .flat_map(|item| match &item.content {
            Content::Controls(buttons) => vec![
                buttons
                    .iter()
                    .map(|button| button.text.clone())
                    .collect::<String>(),
            ],
            Content::Text(text, _) => text.lines.iter().map(ToString::to_string).collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn an_answers_marks_show_below_it_and_expand_to_their_lines() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(|events| {
        ExploreComponent::with_keymap(events, comment_editor::KeymapSetting::default())
    });
    let component = bus.get_mut::<ExploreComponent>(target).unwrap();
    let mut exploration = Exploration::new(Arc::new(Comparison {
        checkpoint: ReviewCheckpoint::new("review", "checkpoint"),
        repository_root: "/tmp".into(),
        files: vec![],
        diffs: vec![],
        context: vec![],
        manifest: vec![],
        sources: vec![],
        base: None,
    }));
    exploration.answers.push(answer());
    exploration.conversation.push(turn("kickoff", None));
    exploration
        .conversation
        .push(turn("request", Some("answer")));
    let mut unanswered = turn("unanswered", Some("answer"));
    unanswered.update.not_relevant = vec![at(SourceSide::New, 50, 52)];
    unanswered.update.reopened = vec![at(SourceSide::New, 9, 9)];
    exploration.conversation.push(unanswered);
    component.exploration = Some(exploration);
    // The answer applied the marks of the question it answered.
    component.marks.insert(
        "kickoff".into(),
        TurnMarks {
            answer: Some("answer".into()),
            not_relevant: vec![at(SourceSide::New, 20, 29)],
            ..TurnMarks::default()
        },
    );
    component.marks.insert(
        "request".into(),
        TurnMarks {
            answer: Some("answer".into()),
            reviewed: vec![at(SourceSide::New, 3, 5), at(SourceSide::Old, 2, 2)],
            not_relevant: vec![at(SourceSide::New, 40, 41)],
            reopened: vec![ReopenedLines {
                location: at(SourceSide::New, 9, 9),
                author: MarkAuthor::Jev,
            }],
            problem: None,
        },
    );
    let palette = ui_theme::Theme::default().palette;
    let layout = |component: &ExploreComponent| {
        let mut layout = ConversationLayout::new(Rect::new(0, 0, 80, 40));
        component.answer_marks(&answer(), &mut layout, palette);
        rows(&layout)
    };

    assert_eq!(
        layout(component),
        [
            "[▸ Marked 10 lines not relevant]",
            "[▸ Marked 4 lines reviewed · 2 lines not relevant · reopened 1 line]"
        ]
    );

    component.toggle_marks(1);

    assert_eq!(
        layout(component),
        [
            "[▸ Marked 10 lines not relevant]",
            "[▾ Marked 4 lines reviewed · 2 lines not relevant · reopened 1 line]",
            "  ✓ src/lib.rs new 3-5",
            "  ✓ src/lib.rs old 2",
            "  – src/lib.rs new 40-41 (not relevant)",
            "  ↺ src/lib.rs new 9",
        ]
    );

    // A question nobody answered yet holds its marks, at the end of its page.
    let pending = |component: &ExploreComponent| {
        let mut layout = ConversationLayout::new(Rect::new(0, 0, 80, 40));
        component.question_marks(0, &mut layout, palette);
        rows(&layout)
    };
    assert_eq!(
        pending(component),
        ["[▸ Will mark 3 lines not relevant · reopen 1 line when you answer]"]
    );

    component.toggle_marks(2);

    assert_eq!(
        pending(component),
        [
            "[▾ Will mark 3 lines not relevant · reopen 1 line when you answer]",
            "  – src/lib.rs new 50-52 (not relevant)",
            "  ↺ src/lib.rs new 9",
        ]
    );
}

#[test]
fn a_summary_names_only_what_changed() {
    use super::MarkTense::Applied;
    let counts = |reviewed_lines, reviewed_files, reopened_lines| review_explore::MarkCounts {
        reviewed_lines,
        reviewed_files,
        reopened_lines,
        ..review_explore::MarkCounts::default()
    };

    assert_eq!(super::marks_summary(counts(0, 0, 0), Applied), "");
    assert_eq!(
        super::marks_summary(counts(0, 0, 2), Applied),
        "Reopened 2 lines"
    );
    assert_eq!(
        super::marks_summary(counts(3, 1, 0), Applied),
        "Marked 3 lines and 1 whole file reviewed"
    );
    let not_relevant = review_explore::MarkCounts {
        not_relevant_lines: 5,
        not_relevant_files: 1,
        ..counts(0, 0, 2)
    };
    assert_eq!(
        super::marks_summary(not_relevant, Applied),
        "Marked 5 lines and 1 whole file not relevant · reopened 2 lines"
    );
    assert_eq!(
        super::marks_summary(not_relevant, super::MarkTense::Pending),
        "Will mark 5 lines and 1 whole file not relevant · reopen 2 lines"
    );
}

#[test]
fn a_reply_keeps_its_label_on_the_first_paragraph_unless_a_block_opens_it() {
    let labelled = |reply| super::ConversationLayout::labelled("Agent", reply);

    assert_eq!(
        labelled("**Bold.** Then text."),
        "Agent: **Bold.** Then text."
    );
    for block in [
        "# Title",
        "- item",
        "* item",
        "1. step",
        "> quote",
        "```\ncode\n```",
        "| a |",
    ] {
        assert_eq!(labelled(block), format!("Agent:\n\n{block}"));
    }
    // A sentence that merely starts with a number is a paragraph.
    assert_eq!(labelled("3 tabs start."), "Agent: 3 tabs start.");
}
